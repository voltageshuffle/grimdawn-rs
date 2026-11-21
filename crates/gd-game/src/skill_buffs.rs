//! Buffs and debuffs other characters put on a character: the `SkillBuff`
//! records an aura pass, a curse or a debuff skill lands on it, each kept
//! as one entry ([`ReceivedBuff`]: record, level, modifiers, caster, time
//! left) in [`ReceivedBuffs`]. The entry is the raw state: its stats count
//! in the holder's profile rebuild (`player::rebuild_profile`,
//! `pets::refresh_profiles`, `monster_skills::rebuild_profiles` through
//! [`ReceivedBuffs::stat_records`]), the buff bar lists it
//! (`crate::buff_bar`), a debuff's attack runs from it ([`run_buffs`]).
//!
//! How a buff gets there (Game.dll):
//! - `SkillActivated::ApplyBuffOnTarget` @0x18051a050 fills a
//!   `SkillBuffTransfer`: the buff record's name (+0), the level (+32), the
//!   skill's devotion level (+36 = skill +208, `Skill::GetDevotionLevel`
//!   @0x180490b40, 0 from the constructor, raised by
//!   `IncrementDevotionLevel`: [`ReceivedBuff::devotion`]), its modifiers
//!   (`Skill::GetModifierInfo` + `GetItemModifierInfo`, +40, a level each at
//!   +32 of a 40-byte entry), the duration in ms (+64), the caster's object
//!   id (+76), the applying skill's record (+80), the item it came from
//!   (+120 / +152); then `Character::BuffTarget` @0x180067720 or, for a
//!   debuff (its `a7`), `Character::DebufTarget` @0x1800677b0.
//! - `Character::DebufTarget`: nothing on an invincible or dead (action
//!   state 21) target; else the target's `SkillServices_Character::
//!   CreateUpdateSkillBuff` (vtable +88); then, unless the caster debuffs
//!   itself, `Character::UnderAttack(caster)` on the target, an empty
//!   `DurationDamageManager` attack (`BeginNewAttack(5)` / `EndAttack`) and
//!   the caster's `SkillServices::CastingDebuf` (the `CastDebuf` autocast
//!   trigger, `crate::autocast`).
//! - Curses: `Skill_AttackBuff::TargetResult` @0x1804b1cc0 (also the class
//!   of `Skill_AttackBuffRadius`): per target `CreateTargetFx`, the
//!   duration = the skill's `skillActiveDuration` with its modifiers'
//!   (`GetSkillModifierData`) × 1000 as int, `ApplyBuffOnTarget(…, level =
//!   GetCurrentLevel, debuff)`, then `ActivateSecondarySkills` with that
//!   target. It makes no attack of its own (the wrapper loads its profile
//!   from the `buffSkillName` table, so radius and duration are the buff
//!   record's).
//! - Projectile debuffs (webs, nets, Blade Trap): `Skill_AttackProjectileDebuf
//!   ::TargetResult` @0x1804c0650 the same per target its projectile hits
//!   (impact and explosion radius), without an attack or secondaries
//!   (`crate::casting::ProjectileDebuf`).
//! - Aura passes (`crate::auras`): `Skill_BuffRadius::TargetResult` the same
//!   way with 1500 ms, so a celestial power's aura carries its devotion level
//!   too (the player's, `DevotionState::devotion_level`; pets' and monsters'
//!   skills: 0, only the player's celestial powers gain devotion experience,
//!   `SkillManager::AddExperience`; PORT: not traced further).
//!
//! The rule ([`ReceivedBuffs::land`]; `CreateUpdateSkillBuff`
//! @0x1805586f0, a list of transfers per character searched by buff name):
//! - none of that name: appended, the buff created ([`Landed::New`]);
//! - one equal in every field (`sub_18055B810`: level, devotion level, modifiers,
//!   duration, caster, flags, skill, item): `SkillBuff::SetTimeToLive`
//!   (duration) on the running buff ([`Landed::Refreshed`]);
//! - same caster, other values: the transfer copied over and the buff made
//!   anew (`CreateRemoveSkillBuffConfigCmd` → `SkillManager::CreateSkillBuff`
//!   destroys the old one first, [`Landed::Replaced`]); a transfer with a
//!   +68 count > 0 would `AddTimeToLive` instead (`ApplyBuffOnTarget` always
//!   passes 0);
//! - another caster, equal values but the caster (`sub_180559E30`): ignored
//!   while the first caster `IsAlive`, else replaced;
//! - another caster, other values: replaced when level + its modifiers'
//!   levels + its devotion level exceed the running one's, else ignored
//!   ([`Landed::Ignored`]).
//!
//! A new `SkillBuff` (also a replacing one) goes active
//! (`SkillBuff::ReceiveActiveUpdate` @0x180526e90): `skillCastSound` at the
//! holder (`crate::combat_sounds::buff_cast`), the record's
//! `charFxPakOtherNames` on another character. It is pushed to the front of
//! the holder's buff list, which the UI lists front to back
//! (`crate::auras` module docs): [`ReceivedBuffs::list`] is newest first.
//! `SkillBuff::Update` @0x180526070 runs its time to live (+375; 0 total =
//! endless) down and at the end has the services remove it
//! (`RemoveSkillBuff`, transfer and buff): `skillUnCastSound`, its effects
//! go.
//!
//! Debuffs (`SkillBuff_Debuf` and the `…Trap` / `…Freeze` subclasses, whose
//! `Update` calls it): `SkillBuff_Debuf::Update` @0x180528560 attacks the
//! holder from the caster every 1000 ms (+387, constructor @0x1805284d0;
//! the timer starts at 0, so a new or replaced debuff attacks on its first
//! update, a refreshed one keeps its cadence): `CollectCombatParameters` of
//! the buff record as the caster's attack, `ParametersCombat` style 4 with
//! the multiplier `max(CalculateDamageModifier(ProbabilityToHit(attack OA,
//! holder DA), rand), 1)` (it never misses) and `Character::AttackTarget`
//! (its autocast triggers included) — [`DebuffTick`] → `crate::combat`
//! (`gd_sim::combat::AttackKind::Debuff`). The caster is looked up as any
//! `Character` object (`sub_180019190`: no life check) and `AttackTarget`
//! only refuses copies (`Entity::IsCopy`) and a dead target for styles ≤ 2,
//! so a dead caster's debuffs keep attacking (its kills included) until its
//! body is removed (`crate::combat::corpse_time`); nothing once it is gone.
//! A debuff gives its holder only its character / defense attributes:
//! `SkillBuff_Debuf`'s vtable leaves `CollectPassiveOffensiveDamageAttributes`
//! and `…OffensiveModifierAttributes` (+1048 / +1056) empty, and
//! `CollectPassiveCharAttributes` @0x1805287f0 adds the profile store at the
//! level and the modifiers' (with the accumulator's +824 flag set; not
//! identified) — so its `offensive*` fields are its attack's, not the
//! holder's.
//!
//! `SkillBuff_DebufRadius::Update` @0x1804a2880 (its own, not
//! `SkillBuff_Debuf`'s; only with the first expansion loaded, PORT: always):
//! on the same 1000 ms timer, the caster's foes (`GetTargetsInRadius` with
//! the caster's faction pack) within the buff's `skillTargetRadius` with its
//! modifiers' (`GetSkillModifierData` +60) of the holder, the holder
//! included, each `Character` among them in line of sight of the holder
//! (vtable +1240 = `Skill::IsTargetInLOS(holder, id)` @0x180497ba0, the
//! monsters' `UseSkill` ray: from the holder's `HeadEffect` to the target's
//! centre of mass, `crate::monster_skills::sight_ray`; others are kept
//! untested), are attacked from the caster like the holder above (style 4;
//! the multiplier rolled per target against the holder's defensive ability:
//! [`DebuffTick::holder`]); the profile's `radiusEffectName` (the level's
//! entry, the last past the list; played before the targets are
//! collected) at the holder each time, targets or not (PORT: no stock buff has one; unit-tested only). It neither traps nor freezes (`SkillBuff::
//! Install`).
//!
//! Projectile debuffs on the holder (`SkillBuff_DebufProjectile`: Evernight's
//! Mark of Darkness, Kurn's Mark of Doom; constructor @0x1804a2490
//! runs `SkillBuff_Debuf`'s, `LoadResources` @0x1804a2510 reads `hideFromUI`
//! and `debufAttackTime` (s → whole ms), `Update` @0x1804a2570, only with
//! Fangs of Asterkarn loaded, PORT: always): `SkillBuff::Update` only (no
//! debuff attack on the holder); while the holder is not dead the debuff
//! timer (+386, from 0) runs down and at 0 is re-armed to `debufAttackTime`
//! (≤ 0: the 1000 ms); with the caster object and the applying skill still
//! there, the caster's `SkillServices::SendCreateProjectile` @0x180558230
//! launches that skill's projectile from the holder's coordinates (to 1 m
//! above them), `GetNumProjectiles` (vtable +808, only
//! `Skill_AttackProjectileDrop` overrides the base's constant 1, so
//! one per tick) of them unless `ProjectileLimit` (`skillProjectileMaximumNumber`
//! live ones; no stock record sets it). The applying `Skill_AttackBuff`'s `CreateProjectile`
//! @0x1804b1f50 gives the projectile the profile's (the buff record's)
//! `projectileDuration` as whole seconds; stock: a `ProjectileAreaEffect`
//! (pools of darkness / ichor) of `projectileExplosionRadius` hitting the
//! caster's foes from the buff record's attack (`casting::spawn_area`).
//! Contagious debuffs (`SkillBuff_Contageous`: Bloody Pox, the plague item
//! skills, Fetid Pool's devotion; constructor @0x180527690 runs
//! `SkillBuff_Debuf`'s, vtable with its empty offensive collectors, so it is
//! a debuff in stats, buff bar and attack, [`is_debuff`] / [`attacks`]):
//! - `Install` @0x1805282d0 (a created buff, after `SetTimeToLive`): the
//!   spread timer (+390) = `contagionInterval` (whole ms, default 1000),
//!   the budget (+391) = `contagionMaxSpread` (default 1), the caster
//!   (+392) = the transfer's; `Character::SetImmuneToPlagueTime(buff,
//!   (int)(total ms × 1.5 + 4000))` on the holder ([`plague_immunity_secs`],
//!   [`ReceivedBuffs::immune`]; `Character::UpdateSelf` runs it down by the
//!   frame, erased at ≤ 0; `CharacterIsDying` clears them all); a monster
//!   holder's controller gets `SetNeedsDeathUpdate` (its buffs update on
//!   its death).
//! - `Update` @0x180527700: the debuff's attack first; then, while
//!   spreading (+221), when the timer runs out or the holder has just died
//!   (`IsAlive` was true last update): timer = `contagionInterval`; the
//!   caster's foes (`GetTargetsInRadius` with the caster's faction pack,
//!   the holder excluded) within `contagionRadius` (default 2; none at ≤ 0)
//!   of the holder that are characters it sees (`IsTargetInLOS`, as for
//!   `SkillBuff_DebufRadius`); nothing if the caster object is gone. Up to
//!   `contagionLimit` (default 1) of them that neither hold that buff
//!   (`Character::IsSkillBuffActive`) nor are immune to it
//!   (`IsImmuneToPlague`) get `Character::DebufTarget(caster, …)` with a
//!   transfer of the buff record, its level, devotion level, modifiers and
//!   applying skill / item, for its `skillActiveDuration` with the
//!   modifiers' (`GetSkillModifierData`) × 1000 as int; each infection
//!   takes one off the budget, which at ≤ 0 stops spreading for good (the
//!   current spread still runs to its limit). A holder that died then has
//!   the buff removed (`SkillServices_Character::RemoveSkillBuff`). Every
//!   infected character's own buff spreads the same way (no generation
//!   limit): only the budget per buff and the immunity bound it.
//!
//! PORT: +221 (`SkillActiveState` byte 5, from the creating transfer) is
//! taken as set for every new buff; `Skill::ResolveValidateTarget` is not
//! modelled (`DebufTarget` refuses dead targets); breakables are no
//! characters; the death spread runs the frame the holder becomes [`Dead`].
//!
//! Dispels ([`DispelOn`]): `Character::DispelSkillBuffs` →
//! `SkillManager::DispelSkillBuffs` @0x18053ccf0 runs vtable +1664 on each
//! received `SkillBuff` (only `SkillBuff_Passive*` / `…BuffImmobilize` have
//! one, `SkillBuff_Passive::DispelBuff`) and +1184 on the skills of its own
//! three lists: +552 the active list (`AddToActiveList` @0x18053c960, from
//! `Skill::SetSkillLevel` / `ReceiveStateUpdate`: every skill with a level,
//! not an item skill), +600 the item skills (`GetItemSkillList`), +864 the
//! sub skills (`GetSubSkillsList`). +1184 is empty in `Skill`; the
//! `SkillActivatedBuffSelf` family's is `DispellSelfBuff` @0x18051b4f0:
//! unless the profile is `notDispelable`, `StopSkill` (+1176), which for
//! `Skill_BuffSelfToggled` @0x1804ec7b0 (its subclasses `…Duration`,
//! `Skill_Shapeshift`, `…Colossus`, `…Shield`, …), `Skill_BuffRadius`
//! @0x1804e9950 (and `…Toggled`), `Skill_BuffAttackRadiusToggled`
//! @0x1804e70b0 (and `…Duration`, `…Lightning`), `Skill_OnHitBuffSelf` /
//! `Skill_OnHitAttackRadius` clears the skill's active flag and sends the
//! inactive `SendSkillActiveUpdate` (`skillDeactivatedSound`): timed self
//! buffs, toggles, damage auras and forms all stop. The classes with
//! `DispellSelfBuff` at +1184 (its xrefs from the vtables):
//! `SkillActivatedBuffSelf`, `Skill_BuffSelf{Toggled,Duration,Sick,Shield,
//! Invulnerable,Immobilize,Colossus}`, `Skill_Shapeshift`,
//! `Skill_BuffRadius{,Toggled}`, `Skill_BuffAttackRadius{Toggled,Duration,
//! Lightning}` and an unnamed vtable @0x180799668 with the toggled damage
//! aura's `StopSkill` (`Skill_BuffAttackRadiusDrop`, a `…Duration` by RTTI),
//! `Skill_OnHitAttackRadius`, `Skill_OnHitBuffSelf` (own `StopSkill`
//! @0x1804ffee0), and `Skill_ChargePotion` / `Skill_Teleport` /
//! `Skill_RefreshCooldown`, whose `StopSkill` is empty. Every other class has
//! an empty +1184, so a dispel leaves it running: the passive procs
//! (`Skill_PassiveOnHitBuffSelf` / `…OnCritBuffSelf` / `…OnLifeBuffSelf` /
//! `…OnHitBuffShield`: Stoneward, Will to Live, …), the secondaries'
//! buffs (`SkillSecondary_BuffSelfDuration`, `…BuffRadius`,
//! `…BuffAttackRadiusDuration`), `Skill_BuffOther`, `Skill_GiveBonus`,
//! pets' and passives' skills ([`stopped_by_dispel`]). The three lists
//! go through the same +1184, so an item-granted skill (+600, filled by
//! `SkillManager::AddItemSkills` @0x180538bc0) stops like a learned one;
//! the sub skills (+864) are only `Skill_Shapeshift::Install`'s
//! `grantedSkills` (`SkillManager::AddSubSkills` @0x180539c40, its one
//! caller; `UnInstall` removes them), every stock one an attack (+1184
//! empty), so a dispel stops none of them. A toggle comes back
//! only by a new use, except the main player's: `SkillManager::Update`
//! @0x180532420 runs each active-list (+552) and item-list (+600) skill's
//! `AutoActivateNow` while `IsDispelBuffActive` (a received
//! `SkillBuff_DispelMagic`) is false, and
//! a toggle's (@0x1804eba50, auto toggle +1521 set by the toggles'
//! constructors) switches it on again ([`auto_toggles`]).
//! `DispelSkillDeBuffs` @0x18053cf00
//! runs +1672 on each received one (`SkillBuff_Debuf::DispelDeBuff`
//! @0x180528880: the debuffs and `SkillBuff_Contageous`); each skips a
//! profile with `notDispelable` (+1280, a wrapper's: its `buffSkillName`
//! record's) and removes the buff through `RemoveSkillBuff` (like running
//! out: `skillUnCastSound`); `DispelDamageOverTime` clears the damage over
//! time (`crate::afflictions`). Callers of `Character::DispelSkillBuffs`:
//! `SkillBuff_DispelMagic::Install` and `Skill_DispelMagic::TargetResult`
//! (on foes).
//! `SkillBuff_DispelMagic` (Nullification curses of heroes / nemeses, an
//! elixir; `Load` @0x1804a2e50: `dispelDamageOverTime`, `dispelFriendly`):
//! a plain `SkillBuff`, landed by its `Skill_AttackBuff` like a debuff
//! ([`lands_as_debuff`]); `Install` @0x1804a2ed0 (when created) with a
//! `Character` caster: on a foe of the caster (`FactionManager::IsFriend`)
//! `DispelSkillBuffs`, then the caster's `AttackTarget` with the buff's
//! `CollectCombatParameters` (style 4); on a friend (the caster itself
//! too) `DispelSkillBuffs` with `dispelFriendly`, else `DispelSkillDeBuffs`,
//! and with `dispelDamageOverTime` `DispelDamageOverTime`. It stays for its
//! duration with its stats like any buff.
//! The port's own skills, each by its cast record's class: the player's
//! timed buffs, learned or item-granted, procs included
//! (`player::Loadout::buffs`, the shared [`TimedBuffs`]), learned toggles (into `SkillBook::dispelled`)
//! and form (the Fangs relic's is `notDispelable`); monsters' and pets'
//! timed buffs and switched-on toggles (`MonsterSkills::dispel`,
//! `Pet::dispel`) and anyone's damage auras (`casting::DamageAuras`).
//! A worn item's toggle stops and comes back like a learned one (into
//! `SkillBook::dispelled`, `crate::player` docs: item toggles). A stopped
//! buff-self skill's absorb ends with it ([`stop_shields`]) and a stopped `Skill_BuffSelfImmobilize`'s
//! hold with its last refresh (`casting::ImmobilizeRefresh`). Death stops every character's timed self buffs the
//! same way (`SkillManager::ImDead`, [`stop_buffs_on_death`]). PORT: the attack's multiplier
//! is the debuff tick's roll ([`DebuffTick`]).
//!
//! Traps and freezes (`SkillBuff_DebufTrap` / `…Freeze`,
//! @0x180528970..0x180528f60): `SetTimeToLive(ms)` (creation and refresh)
//! sets the time left and total, then `ModifyDuration` takes the holder's
//! resistance (`GetResistance`: the sum of its defense attributes of type 44
//! trap / 45 freeze, `defensiveTrap` / `defensiveFreeze`) off the time left
//! when > 0 ([`resisted_ms`]; the total stays, the buff bar's time is left /
//! total); `Install` @0x180528a20 (on creation, new or replaced) plays the
//! holder's stun sound ([`on_created`] → `crate::combat_sounds::StunSound`).
//! `Update` = the debuff's attack, then every 100 ms (+392; timer from 0)
//! while more than 1 ms is left `ApplyBuffDamage`: `DurationDamageManager::
//! AddFixedDamage(44 / 45, 1.0, min(0.25 s (+390), time left))` on the
//! holder, so it is trapped (moves not, acts) / frozen for as long as the
//! buff lasts (`crate::afflictions`, `Afflictions::hold`).
//!
//! The transfer's modifiers are the buff's own (`SkillManager::
//! CreateSkillBuff`: each is created as a skill at its level and added to
//! the buff's modifier list, +320, or its item modifiers), so they add to
//! its attack like any skill's (`Skill::CollectLocalOffensiveDamageAttributes`
//! / `…ModifierAttributes` → `AddModifierOffensive*Attributes`):
//! [`DebuffTick::mods`].
//!
//! PORT: the per-debuff random stream (+389) is the combat `Rng`; the empty
//! `DurationDamageManager` attack is not made; the player's own buffs on
//! itself stay in `Loadout::buffs` / `SkillBook::stat_sources` (see
//! `crate::auras`).
use crate::afflictions::Afflictions;
use crate::casting::{BuffFx, level_f};
use crate::character::Character;
use crate::collision::WorldCollision;
use crate::combat::{Combatant, Dead};
use crate::data::GameData;
use crate::player::ProfileDirty;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;
use gd_sim::attributes::CombatType;
use std::collections::HashMap;

pub struct SkillBuffsPlugin;

impl Plugin for SkillBuffsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DebufTarget>()
            .add_message::<DebuffTick>()
            .add_message::<DispelOn>()
            .add_message::<DispelInstall>()
            .add_systems(
                Update,
                (
                    run_buffs,
                    install_dispels,
                    dispel_on,
                    stop_buffs_on_death,
                    auto_toggles,
                )
                    .chain()
                    .after(crate::casting::CastingSet)
                    .before(crate::auras::AurasSet)
                    .before(crate::combat::CombatSet),
            );
    }
}

/// `SkillBuff_Debuf` +387: ms between a debuff's attacks.
pub const DEBUFF_TICK_SECS: f32 = 1.0;

/// One buff another character keeps on this one (the engine's
/// `SkillBuffTransfer` with its running `SkillBuff`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedBuff {
    /// The buff record (`buffSkillName`: stats, icon, `hideFromUI`).
    pub buff: String,
    /// The skill that put it here (the aura, the curse; tooltip).
    pub skill: String,
    pub level_index: usize,
    /// The caster.
    pub from: Entity,
    /// The caster's modifiers of the skill (record, level index).
    pub mods: Vec<(String, usize)>,
    /// The applying skill's devotion level (the transfer's +36, module
    /// docs; 0 but for a celestial power).
    pub devotion: u32,
    /// Seconds until it runs out.
    pub left: f32,
    /// The duration it was given (s; 0 = endless).
    pub total: f32,
    /// A debuff's next attack (s; None: not an attacking debuff).
    pub tick: Option<f32>,
    /// A trap / freeze debuff's hold: its type and next refresh (s).
    pub hold: Option<(CombatType, f32)>,
    /// A `SkillBuff_Contageous`'s spread (None: another class).
    pub contagion: Option<Contagion>,
    /// Seconds to the buff's next own bonus (`SkillBuff::Update` +382: 0 when created, then 1 s;
    /// `crate::casting::Bonus::buff_duration`).
    pub heal: f32,
}

/// A `SkillBuff_Contageous` entry's spread state (module docs: +390 timer,
/// +391 budget, +221 spreading).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contagion {
    /// Seconds to the next spread.
    pub next: f32,
    /// Infections left (`contagionMaxSpread` at creation).
    pub left: i32,
    /// Still spreading (cleared once `left` reaches 0 in a spread).
    pub spreading: bool,
}

impl Contagion {
    /// `SkillBuff_Contageous::Install`: the first spread after
    /// `contagionInterval`, `contagionMaxSpread` infections.
    fn install(rec: &Record, level_index: usize) -> Self {
        let p = contagion_profile(rec, level_index);
        Self {
            next: p.interval,
            left: p.max_spread,
            spreading: true,
        }
    }
}

/// A contagious buff record's `contagion*` values at a level (the
/// `SkillProfile::GetContagion*` getters: the level's entry, the last one
/// past the end, defaults without values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContagionProfile {
    /// `contagionInterval` (s; whole ms, default 1 s).
    pub interval: f32,
    /// `contagionRadius` (m; default 2).
    pub radius: f32,
    /// `contagionLimit`: infections per spread (default 1).
    pub limit: i32,
    /// `contagionMaxSpread`: infections in all (default 1).
    pub max_spread: i32,
}

pub fn contagion_profile(rec: &Record, level_index: usize) -> ContagionProfile {
    let v = |k: &str| gd_sim::stats::level_value(rec, k, level_index);
    ContagionProfile {
        interval: v("contagionInterval").map_or(1.0, |s| (s * 1000.0) as i32 as f32 / 1000.0),
        radius: v("contagionRadius").unwrap_or(2.0),
        limit: v("contagionLimit").map_or(1, |n| n as i32),
        max_spread: v("contagionMaxSpread").map_or(1, |n| n as i32),
    }
}

/// `SkillBuff_Contageous::Install`: the holder's plague immunity to that
/// buff, (int)(its total ms × 1.5 + 4000) ms.
pub fn plague_immunity_secs(total: f32) -> f32 {
    ((total * 1000.0).round() * 1.5 + 4000.0) as i32 as f32 / 1000.0
}

impl ReceivedBuff {
    /// A buff of `total` seconds, attacking when its record is an attacking
    /// debuff ([`attacks`]).
    pub fn new(
        db: &Database,
        buff: &str,
        skill: &str,
        level_index: usize,
        from: Entity,
        mods: Vec<(String, usize)>,
        total: f32,
    ) -> Self {
        let rec = db.get(buff);
        let tick = rec.is_some_and(attacks).then_some(0.0);
        let hold = rec.and_then(holds).map(|t| (t, 0.0));
        let contagion = rec
            .filter(|r| r.class == CONTAGEOUS)
            .map(|r| Contagion::install(r, level_index));
        Self {
            buff: buff.to_owned(),
            skill: skill.to_owned(),
            level_index,
            from,
            mods,
            devotion: 0,
            left: total,
            total,
            tick,
            hold,
            contagion,
            heal: 0.0,
        }
    }

    /// The level sum the "other caster" rule compares (module docs).
    fn strength(&self) -> usize {
        self.level_index
            + 1
            + self.mods.iter().map(|m| m.1 + 1).sum::<usize>()
            + self.devotion as usize
    }

    /// Equal but for the caster (`sub_180559E30`).
    fn same_values(&self, o: &ReceivedBuff) -> bool {
        self.level_index == o.level_index
            && self.devotion == o.devotion
            && self.mods == o.mods
            && self.total == o.total
            && self.skill.eq_ignore_ascii_case(&o.skill)
    }
}

/// A debuff record whose `Update` attacks (module docs): its holder, or
/// for a `SkillBuff_DebufRadius` the caster's foes around it.
/// A `SkillBuff_DebufProjectile` launches its projectile from the holder on
/// that timer instead ([`DEBUF_PROJECTILE`]).
pub fn attacks(rec: &Record) -> bool {
    matches!(
        rec.class.as_str(),
        "SkillBuff_Debuf"
            | "SkillBuff_DebufTrap"
            | "SkillBuff_DebufFreeze"
            | "SkillBuff_DebufRadius"
            | CONTAGEOUS
            | DEBUF_PROJECTILE
    )
}

/// The projectile-launching debuff class (module docs).
pub const DEBUF_PROJECTILE: &str = "SkillBuff_DebufProjectile";

/// A `SkillBuff_DebufProjectile`'s launch due: the caster, the holder's
/// position, the buff record and level.
struct Launch {
    caster: Entity,
    at: Vec3,
    buff: String,
    level_index: usize,
    hostile: bool,
}

/// The dispelling buff class (module docs).
pub const DISPEL_MAGIC: &str = "SkillBuff_DispelMagic";

/// A received buff `Character::DispelSkillBuffs` removes: the classes with
/// `SkillBuff_Passive::DispelBuff` (module docs).
fn dispellable_buff(rec: &Record) -> bool {
    matches!(
        rec.class.as_str(),
        "SkillBuff_Passive"
            | "SkillBuff_PassiveEndless"
            | "SkillBuff_PassiveCharged"
            | "SkillBuff_PassiveShield"
            | "SkillBuff_BuffImmobilize"
    )
}

/// The contagious debuff class (module docs).
pub const CONTAGEOUS: &str = "SkillBuff_Contageous";

/// What a trap / freeze debuff holds its holder in (module docs).
pub fn holds(rec: &Record) -> Option<CombatType> {
    match rec.class.as_str() {
        "SkillBuff_DebufTrap" => Some(CombatType::Trap),
        "SkillBuff_DebufFreeze" => Some(CombatType::Freeze),
        _ => None,
    }
}

/// `SkillBuff_DebufTrap` +392 / +390: ms between holds, the longest hold.
pub const HOLD_EVERY_SECS: f32 = 0.1;
pub const HOLD_SECS: f32 = 0.25;

/// A `SkillBuff_Debuf*` record (`SkillBuff_Contageous` is one too: its
/// constructor runs `SkillBuff_Debuf`'s, module docs).
pub fn is_debuff(rec: &Record) -> bool {
    is_debuff_class(&rec.class)
}

/// A buff record a curse wrapper lands on its targets through
/// `Character::DebufTarget` rather than hitting them: the debuffs and the
/// dispelling buff (Nullification's `SkillBuff_DispelMagic`, module docs).
pub fn lands_as_debuff(rec: &Record) -> bool {
    is_debuff(rec) || rec.class == DISPEL_MAGIC
}

/// [`is_debuff`] by class name.
pub fn is_debuff_class(class: &str) -> bool {
    class.starts_with("SkillBuff_Debuf") || class == CONTAGEOUS
}

/// The timed self buffs of one character — the one store the player
/// ([`crate::player::Loadout::buffs`]), monsters
/// ([`crate::monster_skills::MonsterSkills::buffs`]) and pets
/// ([`crate::pets::Pet::buffs`]) share: (skill record, level index, seconds
/// left), plus the learned modifiers applied with each buff
/// (`SkillBuff::CollectPassiveCharAttributes` @0x180526a20 adds the level
/// stats and `Skill::AddModifierCharAttributes` while active).
/// The engine keeps one `SkillBuff` list per character; the stats it gives
/// derive from the list at the profile rebuild ([`Self::stat_records`], no
/// cached copy), and one system ticks and expires every character's list
/// (`casting::expire_buffs`); death stops the timed entries ([`stop_buffs_on_death`]). A shapeshift form is an
/// entry with no end (`f32::INFINITY`): it leaves with the form, not by expiry, dispel or death.
#[derive(Component, Clone, Debug, Default)]
pub struct TimedBuffs {
    pub list: Vec<(String, usize, f32)>,
    /// Buff → its learned modifiers (record, level index); the player's
    /// (monsters and pets leave it empty).
    pub mods: HashMap<String, Vec<(String, usize)>>,
}

impl TimedBuffs {
    /// A buff starts (or starts again) under `skill`'s name; the old
    /// entry's modifiers go with it.
    pub fn add(&mut self, skill: &str, li: usize, secs: f32) {
        self.list.retain(|b| b.0 != skill);
        self.mods.remove(skill);
        self.list.push((skill.to_owned(), li, secs));
    }

    /// Whether `skill`'s buff runs.
    pub fn running(&self, skill: &str) -> bool {
        self.list.iter().any(|b| b.0.eq_ignore_ascii_case(skill))
    }

    /// Runs the list down by `dt`; the expired names return (their effects
    /// and modifiers go, the caller plays the sounds and marks the profile
    /// dirty). A form entry (`f32::INFINITY`) never expires.
    pub fn tick(&mut self, dt: f32) -> Vec<String> {
        for b in &mut self.list {
            b.2 -= dt;
        }
        let gone: Vec<String> = self
            .list
            .iter()
            .filter(|b| b.2 <= 0.0)
            .map(|b| b.0.clone())
            .collect();
        if !gone.is_empty() {
            self.list.retain(|b| b.2 > 0.0);
            for g in &gone {
                self.mods.remove(g);
            }
        }
        gone
    }

    /// Ends every timed entry at once (death's `StopSkill`, [`stop_buffs_on_death`]); the ended names return as
    /// [`Self::tick`]'s do. A form entry (`f32::INFINITY`) stays: it leaves with the form.
    pub fn stop_timed(&mut self) -> Vec<String> {
        for b in self.list.iter_mut().filter(|b| b.2.is_finite()) {
            b.2 = 0.0;
        }
        self.tick(0.0)
    }

    /// The stats the list gives: each entry's profile record (a wrapper's
    /// `buffSkillName`, else the entry itself) and its learned modifiers',
    /// at their levels — what the profile rebuild sums.
    pub fn stat_records(&self, db: &Database) -> Vec<(Record, usize)> {
        let mut out = Vec::new();
        for (s, li, _) in &self.list {
            let Some(r) = db.get_owned(s) else { continue };
            match gd_sim::skills::profile_name(&r).and_then(|b| db.get_owned(b)) {
                Some(p) => out.push((p, *li)),
                None => out.push((r, *li)),
            }
            if let Some(mods) = self.mods.get(s) {
                for (m, ml) in mods {
                    if let Some(r) = db.get_owned(m) {
                        out.push((r, *ml));
                    }
                }
            }
        }
        out
    }
}

/// The buffs other characters keep on this one, newest first (module docs).
#[derive(Component, Default, Clone, Debug)]
pub struct ReceivedBuffs {
    pub list: Vec<ReceivedBuff>,
    /// `Character::SetImmuneToPlagueTime`: contagious buff records this
    /// character can't catch by spread for the seconds left (module docs).
    pub immune: Vec<(String, f32)>,
    /// A buff came or went since the profile was last built.
    pub(crate) changed: bool,
}

/// What landing a buff did ([`ReceivedBuffs::land`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landed {
    /// Not on before: created.
    New,
    /// The running one made anew with the new values.
    Replaced,
    /// The running one's time reset.
    Refreshed,
    /// The running one kept; the new one dropped.
    Ignored,
}

impl Landed {
    /// A `SkillBuff` was created (it goes active: cast sound, effects).
    pub fn created(self) -> bool {
        matches!(self, Landed::New | Landed::Replaced)
    }
}

impl ReceivedBuffs {
    /// The stat records: each buff at its level, then its modifiers,
    /// without their energy reserve (`characterManaLimitReserve…`), a
    /// debuff's also without its `offensive*` fields (module docs).
    /// A `SkillBuff` adds its profile's `CharAttributeStore_Skill`
    /// (+496), which reads no reserve field; the reserve is the toggle's
    /// penalty store on the caster (`SkillBook::stat_sources`). A pet under
    /// the player's Elemental Infusion shows a full energy bar in the real
    /// game.
    pub fn stat_records(&self, db: &Database) -> Vec<(Record, usize)> {
        let strip = |mut r: Record, debuff: bool| {
            std::sync::Arc::make_mut(&mut r.fields).retain(|(k, _)| {
                !k.starts_with("characterManaLimitReserve")
                    && !(debuff && k.starts_with("offensive"))
            });
            r
        };
        let mut out = Vec::new();
        for b in &self.list {
            let Some(r) = db.get_owned(&b.buff) else {
                continue;
            };
            let debuff = is_debuff(&r);
            out.push((strip(r, debuff), b.level_index));
            out.extend(
                b.mods
                    .iter()
                    .filter_map(|(m, li)| db.get_owned(m).map(|r| (strip(r, debuff), *li))),
            );
        }
        out
    }

    /// Whether `buff` (a record name) is on.
    pub fn has(&self, buff: &str) -> bool {
        self.list.iter().any(|b| b.buff.eq_ignore_ascii_case(buff))
    }

    /// `Character::IsImmuneToPlague(buff)`.
    pub fn is_immune(&self, buff: &str) -> bool {
        self.immune
            .iter()
            .any(|i| i.0.eq_ignore_ascii_case(buff) && i.1 > 0.0)
    }

    /// `Character::SetImmuneToPlagueTime(buff, secs)`: set or replaced.
    fn set_immune(&mut self, buff: &str, secs: f32) {
        match self
            .immune
            .iter_mut()
            .find(|i| i.0.eq_ignore_ascii_case(buff))
        {
            Some(i) => i.1 = secs,
            None => self.immune.push((buff.to_owned(), secs)),
        }
    }

    /// `CreateUpdateSkillBuff` (module docs). `alive`: whether a caster is
    /// still alive.
    pub fn land(&mut self, b: ReceivedBuff, alive: impl Fn(Entity) -> bool) -> Landed {
        let Some(i) = self
            .list
            .iter()
            .position(|x| x.buff.eq_ignore_ascii_case(&b.buff))
        else {
            self.list.insert(0, b);
            self.changed = true;
            return Landed::New;
        };
        let old = &mut self.list[i];
        let replace = if old.from == b.from {
            if old.same_values(&b) {
                old.left = b.total;
                return Landed::Refreshed;
            }
            true
        } else if old.same_values(&b) {
            !alive(old.from)
        } else {
            b.strength() > old.strength()
        };
        if !replace {
            return Landed::Ignored;
        }
        self.list.remove(i);
        self.list.insert(0, b);
        self.changed = true;
        Landed::Replaced
    }

    /// Takes the "changed" flag.
    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}

/// `Character::DebufTarget` (module docs): put `buff` on `target`.
#[derive(Message, Clone, Debug)]
pub struct DebufTarget {
    pub caster: Entity,
    pub target: Entity,
    pub buff: String,
    /// The skill applying it (the curse).
    pub skill: String,
    pub level_index: usize,
    pub mods: Vec<(String, usize)>,
    /// The skill's devotion level ([`ReceivedBuff::devotion`]).
    pub devotion: u32,
    /// Seconds (the transfer's whole milliseconds).
    pub secs: f32,
}

/// A debuff's attack on its holder (module docs), sent by [`run_buffs`] and
/// made by `crate::combat` as the caster's `TakeAttack` style 4.
#[derive(Message, Clone, Debug)]
pub struct DebuffTick {
    pub caster: Entity,
    pub target: Entity,
    /// The debuff's holder: its defensive ability rolls the multiplier
    /// (the target but for a `SkillBuff_DebufRadius`'s other targets).
    pub holder: Entity,
    pub buff: String,
    pub level_index: usize,
    /// The buff's modifiers (the transfer's; module docs).
    pub mods: Vec<(String, usize)>,
}

/// What a created buff does on landing (module docs): `skillCastSound` at
/// the holder, a trap / freeze's stun sound; on another character than the
/// caster, when new, the buff record's (else the skill's) `charFxPakOtherNames`.
/// Its heals are the casting skill's bonus and the buff's own every second
/// (`crate::casting::Bonus`, [`ReceivedBuff::heal`]).
pub(crate) fn on_created(
    commands: &mut Commands,
    data: &GameData,
    b: &ReceivedBuff,
    target: Entity,
    at: Vec3,
    put: Landed,
) {
    if let Some(pak) = crate::combat_sounds::buff_cast(data, &b.buff, true) {
        debug!(
            "buff {} (level {}) on {target} -> {pak}",
            b.buff.rsplit('/').next().unwrap_or(""),
            b.level_index + 1
        );
        commands.write_message(crate::visuals::PlaySound { pak, at: Some(at) });
    }
    // `SkillBuff_DebufTrap::Install` (freezes too): the holder's stun sound.
    if b.hold.is_some() {
        commands.write_message(crate::combat_sounds::StunSound { holder: target });
    }
    // `SkillBuff_DispelMagic::Install`.
    if data
        .db
        .get(&b.buff)
        .is_some_and(|r| r.class == DISPEL_MAGIC)
    {
        commands.write_message(DispelInstall {
            caster: b.from,
            holder: target,
            buff: b.buff.clone(),
            level_index: b.level_index,
            mods: b.mods.clone(),
        });
    }
    if put != Landed::New || target == b.from {
        return;
    }
    let brec = data.db.get(&b.buff);
    let pak = brec
        .as_ref()
        .and_then(|r| {
            r.strs_iter("charFxPakOtherNames")
                .next()
                .map(|p| p.to_string())
        })
        .or_else(|| {
            data.db.get(&b.skill).and_then(|r| {
                r.strs_iter("charFxPakOtherNames")
                    .next()
                    .map(|p| p.to_string())
            })
        });
    if let Some(pak) = pak {
        let toggled = data
            .db
            .get(&b.skill)
            .is_some_and(|r| gd_sim::skills::is_toggle(&r.class));
        crate::casting::attach_buff_fx(commands, data, &pak, target, &b.skill, toggled);
    }
}

type Targets<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static Combatant,
        Option<&'static mut ReceivedBuffs>,
        Option<&'static mut Afflictions>,
    ),
    Without<Dead>,
>;
type Others<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static Combatant,
        crate::combat::VictimSide,
    ),
    Without<Dead>,
>;
/// A debuff caster's side (`combat::Side`), looked up as any character.
type Bodies<'w, 's> = Query<'w, 's, crate::combat::SideData, With<Combatant>>;

/// A contagious buff's spread due this frame (module docs).
struct Pulse {
    holder: Entity,
    at: Transform,
    buff: ReceivedBuff,
    /// The holder died: its entry is gone, nothing to write back.
    dying: bool,
}

/// Each frame, before the aura passes (`crate::auras::pulse_auras`): buffs
/// run out (`skillUnCastSound`, their effects on others go), debuffs land
/// ([`DebufTarget`]), debuffs attack ([`DebuffTick`]) and hold (traps,
/// freezes), contagious ones spread. A player whose received buffs changed
/// is marked [`ProfileDirty`] (by `pulse_auras`).
#[allow(clippy::too_many_arguments)]
pub fn run_buffs(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut debufs: MessageReader<DebufTarget>,
    mut targets: Targets,
    others: Others,
    bodies: Bodies,
    fx: Query<(Entity, &BuffFx)>,
    mut ticks: MessageWriter<DebuffTick>,
    (chars, coll): (Query<&'static Character>, Res<WorldCollision>),
    (mut dying, breakables, invincible): (
        Query<(Entity, &Transform, &mut ReceivedBuffs), (With<Dead>, Added<Dead>)>,
        Query<(), With<crate::world::Breakable>>,
        Query<(), With<crate::combat::Invincible>>,
    ),
) {
    let sight = Sight {
        data: &data,
        chars: &chars,
        coll: &coll,
    };
    let dt = time.delta_secs();
    for (e, tf, _, buffs, _) in &mut targets {
        let Some(mut buffs) = buffs else { continue };
        // The timers tick every frame; nothing reads `Changed<ReceivedBuffs>` (the `changed` flag marks real changes).
        let buffs = buffs.bypass_change_detection();
        // `Character::UpdateSelf`: plague immunities run down.
        if !buffs.immune.is_empty() {
            buffs.immune.iter_mut().for_each(|i| i.1 -= dt);
            buffs.immune.retain(|i| i.1 > 0.0);
        }
        buffs
            .list
            .iter_mut()
            .filter(|b| b.total > 0.0)
            .for_each(|b| b.left -= dt);
        // `SkillBuff::Update`: the buff's bonus every second, the first at once.
        for b in buffs.list.iter_mut() {
            b.heal -= dt;
            if b.heal > 0.0 {
                continue;
            }
            b.heal = 1.0;
            if let Some(r) = data.db.get(&b.buff) {
                let mods: Vec<_> = b
                    .mods
                    .iter()
                    .filter_map(|(m, l)| data.db.get(m).map(|r| (r, *l)))
                    .collect();
                let li = b.level_index;
                crate::casting::Bonus::buff_duration(|k| {
                    level_f(r, k, li) + mods.iter().map(|(m, l)| level_f(m, k, *l)).sum::<f32>()
                })
                .queue(&mut commands, e);
            }
        }
        let gone: Vec<ReceivedBuff> = buffs
            .list
            .iter()
            .filter(|b| b.total > 0.0 && b.left <= 0.0)
            .cloned()
            .collect();
        if gone.is_empty() {
            continue;
        }
        buffs.list.retain(|b| b.total <= 0.0 || b.left > 0.0);
        buffs.changed = true;
        // `SkillBuff::ReceiveActiveUpdate` going inactive: `skillUnCastSound`.
        removed(&mut commands, &data, &fx, e, tf.translation, &gone);
    }
    // BTreeMap: the landing order feeds `run_debuffs`' immediate ticks below,
    // so it must be deterministic (a HashMap's would vary run to run). One map
    // for the frame's landings and spreads: each new holder's list is inserted
    // once, at the end, with everything that landed on it.
    let mut fresh: std::collections::BTreeMap<Entity, ReceivedBuffs> =
        std::collections::BTreeMap::new();
    for d in debufs.read() {
        land(
            &mut commands,
            &data,
            &mut targets,
            &others,
            &invincible,
            &mut fresh,
            d,
        );
    }
    // A debuff landing on a character without buffs ticks at once too.
    let mut pulses = Vec::new();
    let mut launches = Vec::new();
    let mut radius_fx = Vec::new();
    for (&t, list) in fresh.iter_mut() {
        if let Ok((_, tf, _, _, aff)) = targets.get_mut(t) {
            run_debuffs(
                &data.db,
                list,
                t,
                tf,
                0.0,
                aff.map(Mut::into_inner),
                &others,
                &bodies,
                &sight,
                &mut ticks,
                &mut pulses,
                &mut launches,
                &mut radius_fx,
            );
        }
    }
    for (t, tf, _, buffs, aff) in &mut targets {
        if let Some(mut b) = buffs {
            run_debuffs(
                &data.db,
                b.bypass_change_detection(),
                t,
                tf,
                dt,
                aff.map(Mut::into_inner),
                &others,
                &bodies,
                &sight,
                &mut ticks,
                &mut pulses,
                &mut launches,
                &mut radius_fx,
            );
        }
    }
    for (fx, at) in radius_fx {
        crate::visuals::spawn_fx(&mut commands, &data, &fx, Transform::from_translation(at));
    }
    // The debuffs' projectiles: an area of the buff record's projectile at
    // the holder for its `projectileDuration` (whole s; `Skill_AttackBuff::
    // CreateProjectile`), radius `projectileExplosionRadius`.
    for l in launches {
        let Some(rec) = data.db.get(&l.buff) else {
            continue;
        };
        let dur = level_f(rec, "projectileDuration", l.level_index) as i32 as f32;
        let radius = level_f(rec, "projectileExplosionRadius", l.level_index);
        let cast = crate::casting::Casting {
            skill: l.buff.clone(),
            level_index: l.level_index,
            kind: crate::casting::Kind::Area,
            point: l.at,
            target: None,
            hostile: l.hostile,
            parent: None,
        };
        crate::casting::spawn_area(&mut commands, &data, rec, cast, l.caster, dur, radius);
    }
    // `SkillBuff_Contageous::Update` on a holder that died: a spread at
    // once, then `RemoveSkillBuff`; `Character::CharacterIsDying` clears
    // its immunities.
    for (e, tf, mut buffs) in &mut dying {
        for b in buffs
            .list
            .iter()
            .filter(|b| b.contagion.is_some_and(|c| c.spreading))
        {
            pulses.push(Pulse {
                holder: e,
                at: *tf,
                buff: b.clone(),
                dying: true,
            });
        }
        if buffs.list.iter().any(|b| b.contagion.is_some()) {
            buffs.list.retain(|b| b.contagion.is_none());
            buffs.changed = true;
        }
        buffs.immune.clear();
    }
    let spreads = spread(
        &data.db,
        &mut targets,
        &others,
        &bodies,
        &breakables,
        &sight,
        &fresh,
        pulses,
    );
    for d in &spreads {
        land(
            &mut commands,
            &data,
            &mut targets,
            &others,
            &invincible,
            &mut fresh,
            d,
        );
    }
    for (t, list) in fresh {
        commands.entity(t).try_insert(list);
    }
}

/// `Character::DebufTarget` → `CreateUpdateSkillBuff` (module docs): lands
/// `d` on its target (in `fresh` when it has no buffs yet).
fn land(
    commands: &mut Commands,
    data: &GameData,
    targets: &mut Targets,
    others: &Others,
    invincible: &Query<(), With<crate::combat::Invincible>>,
    fresh: &mut std::collections::BTreeMap<Entity, ReceivedBuffs>,
    d: &DebufTarget,
) {
    // `Character::DebufTarget`: nothing on an invincible target.
    if invincible.contains(d.target) {
        return;
    }
    let Ok((t, tf, cb, buffs, _)) = targets.get_mut(d.target) else {
        return;
    };
    let b = ReceivedBuff {
        devotion: d.devotion,
        ..ReceivedBuff::new(
            &data.db,
            &d.buff,
            &d.skill,
            d.level_index,
            d.caster,
            d.mods.clone(),
            d.secs,
        )
    };
    let mut buffs = buffs;
    let list = match buffs.as_deref_mut() {
        Some(list) => list,
        None => fresh.entry(t).or_default(),
    };
    let put = list.land(b.clone(), |c| others.contains(c));
    // A trap / freeze's `SetTimeToLive` (new or refreshed) shortens its
    // time by the holder's resistance (module docs).
    if put != Landed::Ignored
        && let Some((ty, _)) = b.hold
        && let Some(entry) = list
            .list
            .iter_mut()
            .find(|x| x.buff.eq_ignore_ascii_case(&b.buff))
    {
        entry.left = resisted_ms(entry.total, resistance(cb, ty)) / 1000.0;
    }
    // `SkillBuff_Contageous::Install` (a created buff): the holder's
    // immunity to its spread.
    if put.created() && b.contagion.is_some() {
        list.set_immune(&b.buff, plague_immunity_secs(b.total));
    }
    debug!(
        "debuff {} L{} from {} on {t}: {put:?}",
        d.buff.rsplit('/').next().unwrap_or(""),
        d.level_index + 1,
        d.caster
    );
    if put.created() {
        on_created(commands, data, &b, t, tf.translation, put);
    }
}

/// `SkillBuff_Contageous::Update`'s spreads (module docs): the infections
/// as [`DebufTarget`]s; the holders' budgets are written back.
#[allow(clippy::too_many_arguments)]
fn spread(
    db: &Database,
    targets: &mut Targets,
    others: &Others,
    bodies: &Bodies,
    breakables: &Query<(), With<crate::world::Breakable>>,
    sight: &Sight,
    fresh: &std::collections::BTreeMap<Entity, ReceivedBuffs>,
    pulses: Vec<Pulse>,
) -> Vec<DebufTarget> {
    let mut out: Vec<DebufTarget> = Vec::new();
    for p in pulses {
        let b = &p.buff;
        // The caster is looked up as any object: gone, no spread.
        let Ok(csd) = bodies.get(b.from) else {
            continue;
        };
        let caster_side = csd.side();
        let Some(rec) = db.get(&b.buff) else { continue };
        let prof = contagion_profile(rec, b.level_index);
        let mut limit = prof.limit;
        let mut left = b.contagion.map_or(0, |c| c.left);
        // The transfer: the buff at its level, devotion level and modifiers
        // for its `skillActiveDuration` with the modifiers' (whole ms).
        let secs = ((level_f(rec, "skillActiveDuration", b.level_index)
            + mod_sum(db, &b.mods, "skillActiveDuration"))
            * 1000.0) as i32 as f32
            / 1000.0;
        if prof.radius > 0.0 {
            for (t, tt, tcb, v) in others.iter() {
                if limit == 0 {
                    break;
                }
                // Foes, not hiding (`FilterInvalidTargets`, `combat::Side::hits`), no breakable.
                if t == p.holder
                    || !caster_side.hits(&v)
                    || breakables.contains(t)
                    || !tcb.in_radius(&tt.compute_affine(), p.at.translation, prof.radius)
                    || !sight.sees(p.holder, &p.at, t, tt)
                {
                    continue;
                }
                // `Character::IsSkillBuffActive` / `IsImmuneToPlague`.
                let held = targets
                    .get(t)
                    .ok()
                    .and_then(|x| x.3)
                    .or_else(|| fresh.get(&t))
                    .is_some_and(|r| r.has(&b.buff) || r.is_immune(&b.buff))
                    || out
                        .iter()
                        .any(|d| d.target == t && d.buff.eq_ignore_ascii_case(&b.buff));
                if held {
                    continue;
                }
                out.push(DebufTarget {
                    caster: b.from,
                    target: t,
                    buff: b.buff.clone(),
                    skill: b.skill.clone(),
                    level_index: b.level_index,
                    mods: b.mods.clone(),
                    devotion: b.devotion,
                    secs,
                });
                left -= 1;
                limit -= 1;
            }
        }
        if p.dying {
            continue;
        }
        if let Ok((_, _, _, Some(mut list), _)) = targets.get_mut(p.holder)
            && let Some(c) = list
                .list
                .iter_mut()
                .find(|x| x.buff.eq_ignore_ascii_case(&b.buff))
                .and_then(|x| x.contagion.as_mut())
        {
            c.left = left;
            if left <= 0 {
                c.spreading = false;
            }
        }
    }
    out
}

/// Whether `DispelSkillBuffs` stops a character's own skill of `class`
/// (module docs, vtables): +1184 is `SkillActivatedBuffSelf::
/// DispellSelfBuff` with a `StopSkill` that ends something. The others
/// (`Skill_Passive*` procs, `SkillSecondary_*` buffs, `Skill_BuffOther`,
/// `Skill_GiveBonus`, `Skill_ChargePotion`, …) keep running.
pub fn stopped_by_dispel(class: &str) -> bool {
    matches!(
        class,
        "Skill_BuffSelfToggled"
            | "Skill_BuffSelfDuration"
            | "Skill_BuffSelfSick"
            | "Skill_BuffSelfShield"
            | "Skill_BuffSelfInvulnerable"
            | "Skill_BuffSelfImmobilize"
            | "Skill_BuffSelfColossus"
            | "Skill_Shapeshift"
            | "Skill_BuffRadius"
            | "Skill_BuffRadiusToggled"
            | "Skill_BuffAttackRadiusToggled"
            | "Skill_BuffAttackRadiusDuration"
            | "Skill_BuffAttackRadiusLightning"
            | "Skill_BuffAttackRadiusDrop"
            | "Skill_OnHitAttackRadius"
            | "Skill_OnHitBuffSelf"
    )
}

/// What a dispel strips from one character (module docs): its buffs
/// (`Character::DispelSkillBuffs`: received `SkillBuff_Passive*` and its
/// own timed buffs), its debuffs (`DispelSkillDeBuffs`) and its damage over
/// time (`DispelDamageOverTime`); none flagged `notDispelable`.
#[derive(Message, Clone, Debug)]
pub struct DispelOn {
    pub target: Entity,
    pub buffs: bool,
    pub debuffs: bool,
    pub dots: bool,
}

/// A `SkillBuff_DispelMagic` was created on `holder` (its `Install`,
/// sent by [`on_created`]).
#[derive(Message, Clone, Debug)]
pub struct DispelInstall {
    pub caster: Entity,
    pub holder: Entity,
    pub buff: String,
    pub level_index: usize,
    pub mods: Vec<(String, usize)>,
}

/// `SkillBuff_DispelMagic::Install` (module docs): on a foe of the caster
/// its buffs go and the caster attacks it with the buff (style 4); on a
/// friend (the caster too) its debuffs go, or with `dispelFriendly` its
/// buffs, and with `dispelDamageOverTime` its damage over time.
fn install_dispels(
    data: Res<GameData>,
    mut installs: MessageReader<DispelInstall>,
    sides: Bodies,
    mut out: MessageWriter<DispelOn>,
    mut ticks: MessageWriter<DebuffTick>,
) {
    for i in installs.read() {
        let (Ok(csd), Ok(hsd)) = (sides.get(i.caster), sides.get(i.holder)) else {
            continue;
        };
        let Some(rec) = data.db.get(&i.buff) else {
            continue;
        };
        if i.caster != i.holder && csd.side().foe(hsd.side_ref()) {
            out.write(DispelOn {
                target: i.holder,
                buffs: true,
                debuffs: false,
                dots: false,
            });
            ticks.write(DebuffTick {
                caster: i.caster,
                target: i.holder,
                holder: i.holder,
                buff: i.buff.clone(),
                level_index: i.level_index,
                mods: i.mods.clone(),
            });
        } else {
            let friendly = rec.bool("dispelFriendly");
            out.write(DispelOn {
                target: i.holder,
                buffs: friendly,
                debuffs: !friendly,
                dots: rec.bool("dispelDamageOverTime"),
            });
        }
    }
}

/// Applies [`DispelOn`]: removed received buffs end like run-out ones
/// (`RemoveSkillBuff`: `skillUnCastSound`, their effects go); the
/// character's own buff-self skills stop (`SkillActivatedBuffSelf::
/// DispellSelfBuff` → `StopSkill`: `skillDeactivatedSound`): timed buffs
/// (the player's through `casting::expire_buffs`), toggles (the player's
/// into `SkillBook::dispelled`, see [`auto_toggles`]), damage auras and the
/// player's form (module docs).
#[allow(clippy::type_complexity)]
fn dispel_on(
    mut commands: Commands,
    data: Res<GameData>,
    mut events: MessageReader<DispelOn>,
    mut q: Query<(
        &Transform,
        Option<&mut ReceivedBuffs>,
        Option<&mut crate::monster_skills::MonsterSkills>,
        Option<&mut Afflictions>,
        Option<&mut crate::player::Loadout>,
        Option<&mut crate::pets::Pet>,
        Option<&mut crate::casting::DamageAuras>,
        Option<&crate::shapeshift::Shapeshifted>,
    )>,
    fx: Query<(Entity, &BuffFx)>,
    mut forms: MessageWriter<crate::shapeshift::Toggle>,
) {
    // A skill of a class whose +1184 does nothing, or with a `notDispelable`
    // profile, keeps running.
    let keep = |name: &str| {
        data.db.get(name).is_some_and(|r| {
            !stopped_by_dispel(&r.class)
                || gd_sim::skills::profile_name(r)
                    .and_then(|p| data.db.get(p))
                    .unwrap_or(r)
                    .bool("notDispelable")
        })
    };
    for d in events.read() {
        let Ok((tf, received, ms, aff, loadout, pet, damage, form)) = q.get_mut(d.target) else {
            continue;
        };
        if let Some(mut list) = received {
            let strip = |b: &ReceivedBuff| {
                data.db.get(&b.buff).is_some_and(|r| {
                    !r.bool("notDispelable")
                        && ((d.buffs && dispellable_buff(r)) || (d.debuffs && is_debuff(r)))
                })
            };
            let gone: Vec<ReceivedBuff> = list.list.iter().filter(|b| strip(b)).cloned().collect();
            if !gone.is_empty() {
                list.list.retain(|b| !strip(b));
                list.changed = true;
                removed(&mut commands, &data, &fx, d.target, tf.translation, &gone);
            }
        }
        if d.buffs {
            let mut stopped: Vec<String> = Vec::new();
            if let Some(mut ms) = ms {
                stopped.extend(ms.dispel(&data.db, keep));
            }
            if let Some(mut p) = pet {
                stopped.extend(p.dispel(keep));
            }
            if let Some(mut a) = damage {
                for s in a.dispel(keep) {
                    if !stopped.contains(&s) {
                        stopped.push(s);
                    }
                }
            }
            // The timed buffs that stop.
            let mut timed: Vec<String> = stopped.clone();
            if let Some(mut l) = loadout {
                // The timed buffs that stop (a form entry stays: it leaves
                // with the form, not by dispel).
                for b in l
                    .buffs
                    .list
                    .iter_mut()
                    .filter(|b| b.2.is_finite() && !keep(&b.0) && b.2 > 0.0)
                {
                    timed.push(b.0.clone());
                    b.2 = 0.0;
                }
                // The learned / granted / worn items' toggles that are on.
                let on: Vec<String> = l
                    .skills_and_item_toggles(&data.db)
                    .iter()
                    .chain(l.granted.iter())
                    .filter(|(s, lv)| {
                        *lv > 0
                            && !keep(s)
                            && l.skill_active(&data.db, s)
                            && data
                                .db
                                .get(s)
                                .is_some_and(|r| gd_sim::skills::is_toggle(&r.class))
                    })
                    .map(|(s, _)| s.clone())
                    .collect();
                if !on.is_empty() {
                    debug!("dispel: {} toggles off", on.len());
                    l.skills
                        .dispelled
                        .extend(on.iter().map(|s| s.to_ascii_lowercase()));
                    commands.entity(d.target).try_insert(ProfileDirty);
                    for s in on {
                        if !stopped.contains(&s) {
                            stopped.push(s);
                        }
                    }
                }
            }
            stop_shields(&mut commands, &data, d.target, &timed);
            if let Some(f) = form.filter(|f| !keep(&f.skill)) {
                forms.write(crate::shapeshift::Toggle {
                    caster: d.target,
                    skill: f.skill.clone(),
                    level_index: 0,
                });
                stopped.push(f.skill.clone());
            }
            for pak in stopped
                .iter()
                .filter_map(|b| crate::combat_sounds::deactivate(&data, b))
            {
                commands.write_message(crate::visuals::PlaySound {
                    pak,
                    at: Some(tf.translation),
                });
            }
        }
        if d.dots
            && let Some(mut a) = aff
        {
            a.clear_dots();
        }
    }
}

/// A stopped `SkillActivatedBuffSelf` skill (`StopSkill` @0x1804ec7b0, the
/// class of `Skill_BuffSelfShield` / `…Immobilize` / `…Duration`)
/// clears its combat filter's on byte (+1560 = the `CombatFilter` at +1528,
/// its +32; `Install` adds the filter once, `ModifyDamage` absorbs while it
/// is on) and its active flag, so a stopped skill with an absorb drops its
/// own buff filter from the caster's [`crate::casting::Shields`] (not a potion's:
/// `Skill_ChargePotion`'s `StopSkill` is empty). A stopped `Skill_BuffSelfImmobilize`
/// refreshes its hold no more: the last refresh runs out (`casting::ImmobilizeRefresh`).
fn stop_shields(commands: &mut Commands, data: &GameData, target: Entity, stopped: &[String]) {
    let absorbs = |r: &Record| {
        ["damageAbsorptionPercent", "damageAbsorption"]
            .iter()
            .any(|k| r.f32s_iter(k).any(|v| v > 0.0))
    };
    let shields: Vec<String> = stopped
        .iter()
        .filter(|s| {
            data.db.get(s).is_some_and(|r| {
                absorbs(r)
                    || gd_sim::skills::profile_name(r)
                        .and_then(|p| data.db.get(p))
                        .is_some_and(absorbs)
            })
        })
        .cloned()
        .collect();
    if !shields.is_empty() {
        commands.queue(move |w: &mut World| {
            if let Some(mut sh) = w.get_mut::<crate::casting::Shields>(target) {
                sh.0.retain(|f| {
                    f.kind != crate::casting::ShieldKind::Buff
                        || !shields.iter().any(|s| s.eq_ignore_ascii_case(&f.skill))
                });
            }
        });
    }
}

/// A character's own timed buffs ended ([`TimedBuffs::tick`] / [`TimedBuffs::stop_timed`]: run out, dispelled,
/// stopped by death): `skillDeactivatedSound` at the caster (`crate::combat_sounds::deactivate`) and the effects
/// they put on it go. The caller marks the profile dirty.
pub(crate) fn timed_buffs_ended(
    commands: &mut Commands,
    data: &GameData,
    fx: &Query<(Entity, &BuffFx)>,
    e: Entity,
    at: Vec3,
    gone: &[String],
) {
    for pak in gone
        .iter()
        .filter_map(|g| crate::combat_sounds::deactivate(data, g))
    {
        commands.write_message(crate::visuals::PlaySound { pak, at: Some(at) });
    }
    for (f, b) in fx {
        if b.owner == e && gone.contains(&b.skill) {
            commands.entity(f).try_despawn();
        }
    }
}

/// Death stops the character's own timed buffs: `Character::ImDying` → `SkillManager::ImDead` `StopSkill`s every
/// active-list and item-list skill (`crate::casting::stop_skills` docs), which ends a timed self buff as a
/// dispel does ([`dispel_on`]): `skillDeactivatedSound`, its effects and absorb ([`stop_shields`]) go, the profile
/// rebuilds. The same for monsters' and pets' lists (`casting::expire_buffs` only runs the living ones' timers). A
/// form entry stays: it leaves with the form.
#[allow(clippy::type_complexity)]
fn stop_buffs_on_death(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &Transform,
            Option<&mut crate::player::Loadout>,
            Option<&mut crate::monster_skills::MonsterSkills>,
            Option<&mut crate::pets::Pet>,
        ),
        Added<Dead>,
    >,
    fx: Query<(Entity, &BuffFx)>,
) {
    for (e, tf, loadout, ms, pet) in &mut q {
        let gone = if let Some(mut l) = loadout {
            let gone = l.bypass_change_detection().buffs.stop_timed();
            if !gone.is_empty() {
                l.set_changed();
                commands.entity(e).try_insert(ProfileDirty);
            }
            gone
        } else if let Some(mut m) = ms {
            let gone = m.bypass_change_detection().buffs.stop_timed();
            if !gone.is_empty() {
                m.mark_profile_dirty();
            }
            gone
        } else if let Some(mut p) = pet {
            let gone = p.bypass_change_detection().buffs_mut().stop_timed();
            if !gone.is_empty() {
                p.mark_profile_dirty();
            }
            gone
        } else {
            continue;
        };
        if gone.is_empty() {
            continue;
        }
        debug!("{e}: death stops {} timed buffs", gone.len());
        stop_shields(&mut commands, &data, e, &gone);
        timed_buffs_ended(&mut commands, &data, &fx, e, tf.translation, &gone);
    }
}

/// The main player's toggles, learned and item ones (Game.dll):
/// `SkillManager::Update` @0x180532420 runs, every
/// update and unless `IsDispelBuffActive` (a received `SkillBuff_DispelMagic`
/// is on), each active-list and item-list skill's `AutoActivateNow`; a
/// toggle's (@0x1804eba50 / `SkillActivatedBuffSelf` @0x18051b890) switches
/// it on when it is off, enabled (level > 0), the character alive, an auto
/// toggle (+1521: every toggle's constructor sets it, nothing clears it),
/// its weapons and hand state qualify and `ExclusiveSkillCheck` passes. So
/// a toggle is on whenever it can be: there is no user switch-off (the hot
/// slots refuse auto toggles: `HotSlotOptionSkill::Validate` @0x18031bb00,
/// `crate::hotbar` docs), a newly learned or loaded one comes on at the next
/// update (the save's active flag does not matter), one stopped by weapons
/// (`toggles_need_weapons`), death or a dispel comes back once its weapons
/// qualify again, after the respawn or once the dispelling buff is gone,
/// with its activate effect.
/// - death: `SkillManager::ImDead` `StopSkill`s every active-list and
///   item-list skill (`skillDeactivatedSound`); the alive test keeps them
///   off until the respawn.
/// - exclusive skills (`exclusiveSkill`, Possession, Primal Bond, Mind over
///   Matter, ...): one going active becomes the manager's exclusive skill
///   (`Skill::HandleExclusiveSkill` @0x18049acf0 → `SetExclusiveSkill`
///   @0x18053fe40, which `StopSkill`s the one before); `ExclusiveSkillCheck`
///   @0x18049ad60 lets another exclusive come on only with a higher level
///   (the points spent, no item bonus), equal: the higher mastery
///   enumeration, equal: the higher item bonus ([`gd_sim::skills::ExclusiveRank`]).
///   So the highest one runs. An item toggle's level is its granted level with no augment
///   (`SkillManager::SetAllSkillAugment` @0x180537230 / `SetSkillLevelAugment` @0x1805375b0 /
///   `FindSkillId` @0x180530ef0 walk only the learned list; no stock augment reaches the 60 item toggles).
///   PORT: several on (a load) keep the first of the book that no later one beats.
#[allow(clippy::type_complexity)]
fn auto_toggles(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            Option<&Transform>,
            &mut crate::player::Loadout,
            Option<Ref<ReceivedBuffs>>,
            Option<Ref<crate::weapon_pool::SkillLinks>>,
            Has<Dead>,
        ),
        With<crate::player::Player>,
    >,
    mut was_dead: Local<bool>,
) {
    let db = &data.db;
    for (e, tf, mut l, received, links, dead) in &mut q {
        // What decides a toggle: the book and worn items (`Loadout`), the weapons
        // (`SkillLinks`), a dispel (`ReceivedBuffs`) and being dead; nothing else
        // changes the outcome, so an idle frame skips the pass. The dispel test is
        // the semantic `changed` flag (a buff came or went), not Bevy's change
        // detection: `run_buffs` ticks the buff timers through `ReceivedBuffs` every
        // frame, which would mark it changed always.
        let was = std::mem::replace(&mut *was_dead, dead);
        if !dead
            && !was
            && !l.is_changed()
            && !links.as_ref().is_some_and(|k| k.is_changed())
            && !received.as_ref().is_some_and(|r| r.changed)
        {
            continue;
        }
        // (record, profile record, learned, item level) of each toggle the player has.
        let toggles: Vec<(String, Record, bool, u32)> = l
            .skills
            .skills
            .iter()
            .filter(|(_, lv)| *lv > 0)
            .map(|(s, _)| (s.clone(), true, 0))
            .chain(l.item_toggles(db).into_iter().map(|(s, lv)| (s, false, lv)))
            .filter_map(|(s, learned, lv)| {
                db.get_owned(&s)
                    .filter(|r| gd_sim::skills::is_toggle(&r.class))
                    .map(|r| (s, r, learned, lv))
            })
            .collect();
        let is_on =
            |l: &crate::player::Loadout, s: &str| !l.skills.switched_off(&s.to_ascii_lowercase());
        let mut stopped: Vec<String> = Vec::new();
        let mut changed = false;
        if dead {
            stopped.extend(
                toggles
                    .iter()
                    .filter(|t| is_on(&l, &t.0))
                    .map(|t| t.0.clone()),
            );
        } else if !received.as_ref().is_some_and(|r| {
            r.list
                .iter()
                .any(|b| db.get(&b.buff).is_some_and(|r| r.class == DISPEL_MAGIC))
        }) {
            // The flag sits on the buff record (`auraconviction1_buff`), like
            // `SkillBook::exclusive_conflict` reads it.
            let exclusive = |s: &str| {
                gd_sim::skills::profile_record(db, s).is_some_and(|r| r.bool("exclusiveSkill"))
            };
            let trees = data.skill_trees();
            let book = l.effective_skills(db, &trees);
            let ranks: Vec<gd_sim::skills::ExclusiveRank> = toggles
                .iter()
                .map(|(s, _, learned, item)| gd_sim::skills::ExclusiveRank {
                    level: if *learned { l.skills.level(s) } else { *item },
                    mastery: trees.mastery_enumeration(s),
                    augment: if *learned {
                        book.bonus
                            .get(&s.to_ascii_lowercase())
                            .copied()
                            .unwrap_or(0)
                    } else {
                        0
                    },
                })
                .collect();
            let rank = |s: &str| {
                toggles
                    .iter()
                    .position(|t| t.0 == s)
                    .map(|i| ranks[i])
                    .expect("a listed toggle")
            };
            // The exclusive skill on (several only from a load: the best
            // stays).
            let mut current: Option<(String, gd_sim::skills::ExclusiveRank)> = None;
            for (s, _, _, _) in toggles
                .iter()
                .filter(|t| exclusive(&t.0) && is_on(&l, &t.0))
            {
                let lv = rank(s);
                match &current {
                    Some((c, clv)) if lv.beats(clv) => {
                        stopped.push(c.clone());
                        current = Some((s.clone(), lv));
                    }
                    Some(_) => stopped.push(s.clone()),
                    None => current = Some((s.clone(), lv)),
                }
            }
            for (s, r, learned, _) in &toggles {
                let low = s.to_ascii_lowercase();
                if is_on(&l, s) || stopped.iter().any(|x| x.eq_ignore_ascii_case(s)) {
                    continue;
                }
                // PORT: item toggles' weapons are not tested (`crate::player`
                // docs).
                if *learned && links.as_ref().is_some_and(|k| !k.weapons_allow(db, r, s)) {
                    continue;
                }
                if exclusive(s) {
                    let lv = rank(s);
                    match &current {
                        Some((_, clv)) if !lv.beats(clv) => continue,
                        Some((c, _)) => stopped.push(c.clone()),
                        None => {}
                    }
                    current = Some((s.clone(), lv));
                }
                l.skills.inactive.remove(&low);
                l.skills.dispelled.remove(&low);
                debug!("auto toggle: {s} on");
                changed = true;
            }
        }
        for s in &stopped {
            l.skills.inactive.insert(s.to_ascii_lowercase());
            if let Some(pak) = crate::combat_sounds::deactivate(&data, s) {
                commands.write_message(crate::visuals::PlaySound {
                    pak,
                    at: tf.map(|t| t.translation),
                });
            }
            debug!("toggle {s} stopped (death / exclusive)");
        }
        if changed || !stopped.is_empty() {
            // The toggles' aura loops come and go with them (`skill_fx`).
            commands.entity(e).try_insert(ProfileDirty);
        }
    }
}

/// Received buffs that ended (ran out, dispelled): `skillUnCastSound` at
/// the holder, the effects they put on it go.
fn removed(
    commands: &mut Commands,
    data: &GameData,
    fx: &Query<(Entity, &BuffFx)>,
    e: Entity,
    at: Vec3,
    gone: &[ReceivedBuff],
) {
    for pak in gone
        .iter()
        .filter_map(|g| crate::combat_sounds::buff_cast(data, &g.buff, false))
    {
        commands.write_message(crate::visuals::PlaySound { pak, at: Some(at) });
    }
    for (f, b) in fx {
        if b.owner == e && gone.iter().any(|g| g.from != e && g.skill == b.skill) {
            commands.entity(f).try_despawn();
        }
    }
}

/// The modifiers' values of `field` (`GetSkillModifierData`).
fn mod_sum(db: &Database, mods: &[(String, usize)], field: &str) -> f32 {
    mods.iter()
        .filter_map(|(m, li)| db.get(m).map(|m| level_f(m, field, *li)))
        .sum()
}
/// The holder's resistance to a hold (`SkillBuff_DebufTrap::GetResistance`:
/// the sum of its defense attributes of that type).
fn resistance(cb: &Combatant, ty: CombatType) -> f32 {
    cb.profile
        .resistances
        .iter()
        .filter(|r| r.0 == ty)
        .map(|r| r.1)
        .sum()
}

/// `SkillBuff_DebufTrap::ModifyDuration`: `secs` (whole ms) less `res` %
/// when `res` > 0, as whole ms.
pub fn resisted_ms(secs: f32, res: f32) -> f32 {
    let ms = (secs * 1000.0).round();
    if res > 0.0 {
        (ms - ms * res * 0.01) as i32 as f32
    } else {
        ms
    }
}

/// Runs the debuffs' attack and hold timers on `holder` at `at` (module
/// docs).
#[allow(clippy::too_many_arguments)]
fn run_debuffs(
    db: &Database,
    buffs: &mut ReceivedBuffs,
    holder: Entity,
    tf: &Transform,
    dt: f32,
    mut aff: Option<&mut Afflictions>,
    others: &Others,
    bodies: &Bodies,
    sight: &Sight,
    out: &mut MessageWriter<DebuffTick>,
    pulses: &mut Vec<Pulse>,
    launches: &mut Vec<Launch>,
    radius_fx: &mut Vec<(String, Vec3)>,
) {
    let at = tf.translation;
    let holder_at = others.get(holder).ok().map(|o| *o.1);
    for b in buffs.list.iter_mut() {
        if let Some((ty, t)) = b.hold.as_mut() {
            *t -= dt;
            if *t <= 0.0 && b.left > 0.001 {
                *t = HOLD_EVERY_SECS;
                if let Some(a) = aff.as_deref_mut() {
                    a.hold(*ty, HOLD_SECS.min(b.left));
                }
            }
        }
        // `SkillBuff_Contageous::Update` after the debuff's: every
        // `contagionInterval` while it spreads.
        if let Some(c) = b.contagion.as_mut() {
            c.next -= dt;
            if c.spreading && c.next <= 0.0 {
                c.next = db
                    .get(&b.buff)
                    .map_or(1.0, |r| contagion_profile(r, b.level_index).interval);
                pulses.push(Pulse {
                    holder,
                    at: *tf,
                    buff: b.clone(),
                    dying: false,
                });
            }
        }
        let Some(t) = b.tick.as_mut() else { continue };
        *t -= dt;
        if *t > 0.0 {
            continue;
        }
        *t = DEBUFF_TICK_SECS;
        let Ok(csd) = bodies.get(b.from) else {
            continue;
        };
        let caster_side = csd.side();
        let Some(rec) = db.get(&b.buff) else { continue };
        if rec.class == DEBUF_PROJECTILE {
            // `SkillBuff_DebufProjectile::Update`: every `debufAttackTime`.
            let every = rec.f32("debufAttackTime").unwrap_or(0.0);
            *t = if every > 0.0 {
                (every * 1000.0) as i32 as f32 / 1000.0
            } else {
                DEBUFF_TICK_SECS
            };
            launches.push(Launch {
                caster: b.from,
                at,
                buff: b.buff.clone(),
                level_index: b.level_index,
                hostile: caster_side.hostile(),
            });
            continue;
        }
        if rec.class != "SkillBuff_DebufRadius" {
            out.write(DebuffTick {
                caster: b.from,
                target: holder,
                holder,
                buff: b.buff.clone(),
                level_index: b.level_index,
                mods: b.mods.clone(),
            });
            continue;
        }
        // `SkillBuff_DebufRadius::Update`: the profile's `radiusEffectName` of the level (the last when
        // the level is past the list) at the holder, then the caster's foes within the
        // buff's `skillTargetRadius` (with its modifiers') of the holder,
        // the holder included, that the holder sees.
        if let Some(fx) = radius_effect(rec, b.level_index) {
            radius_fx.push((fx, at));
        }
        let r = level_f(rec, "skillTargetRadius", b.level_index)
            + mod_sum(db, &b.mods, "skillTargetRadius");
        for (t, tt, tcb, v) in others.iter() {
            if caster_side.hits(&v)
                && t != b.from
                && tcb.in_radius(&tt.compute_affine(), at, r)
                && holder_at.is_none_or(|h| sight.sees(holder, &h, t, tt))
            {
                out.write(DebuffTick {
                    caster: b.from,
                    target: t,
                    holder,
                    buff: b.buff.clone(),
                    level_index: b.level_index,
                    mods: b.mods.clone(),
                });
            }
        }
    }
}

/// A `SkillBuff_DebufRadius` profile's `radiusEffectName` for a level (`Update` @0x1804a2880 clamps the level
/// to the list's last entry; an empty name plays nothing).
fn radius_effect(rec: &gd_formats::arz::Record, level_index: usize) -> Option<String> {
    rec.strs_iter("radiusEffectName")
        .nth(level_index)
        .or_else(|| rec.strs_iter("radiusEffectName").last())
        .filter(|f| !f.is_empty())
        .map(|f| f.to_string())
}

/// What a holder's `IsTargetInLOS` needs (module docs).
struct Sight<'a, 'w, 's> {
    data: &'a GameData,
    chars: &'a Query<'w, 's, &'static Character>,
    coll: &'a WorldCollision,
}

impl Sight<'_, '_, '_> {
    /// `Skill::IsTargetInLOS(holder, target)` (`crate::monster_skills::
    /// sight_ray`); a target that is no character is not tested.
    fn sees(&self, holder: Entity, at: &Transform, target: Entity, tat: &Transform) -> bool {
        let (Ok(ch), Ok(tch)) = (self.chars.get(holder), self.chars.get(target)) else {
            return true;
        };
        let (head, aim) = crate::monster_skills::sight_ray(self.data, ch, at, tat, Some(tch));
        self.coll.los_clear(head, aim)
    }
}

/// Marks the player [`ProfileDirty`] when its received buffs changed.
pub(crate) fn mark_player(
    commands: &mut Commands,
    e: Entity,
    buffs: Option<Mut<ReceivedBuffs>>,
    player: bool,
) {
    if player
        && let Some(mut b) = buffs
        && b.take_changed()
    {
        commands.entity(e).try_insert(ProfileDirty);
    }
}
