//! Damage over time, debuffs and crowd control on characters, after `DurationDamageManager` (Game.dll):
//!
//! - DoTs (`AddDamage`): per damage type a 100 ms timeline, `dps × 0.1` per
//!   slot for the duration; the same source keeps the larger value, different
//!   sources stack (`gd_sim::combat::DotManager`). The source is the
//!   owning object + attribute — the item instance / skill record / passive,
//!   not the attacker (`DamageAttributeDur::AddDamageToAccumulator`
//!   @0x180190560: `(GetObjectId(owner), attribute_index)`;
//!   `gd_sim::actor::Profile::dot_sources`), so weapon + skill + passive DoTs
//!   of one type add up and only a re-application of the same source keeps
//!   max. The attacker rides along for the kill credit (below).
//! - Debuffs (`AddFixedDamage`, slows, OA/DA/armor/resistance/damage
//!   reductions, fumble): per type every slot keeps the largest value, so the
//!   effect is the largest active one (`GetFixedDamage`). They are
//!   not scaled by critical hits (`CombatAttributeDurFixedDamage`).
//! - Type 61 (`offensiveSlowDamageMult*`, `CombatAttributeDur_DamageMultiplier::Execute` @0x180114dd0):
//!   a fixed entry of value % and duration s. Every application is its own
//!   entry and the active value is the largest (`GetFixedDamage`'s per-slot
//!   `fmaxf`, `AddFixedDamage` @0x18027a270 → `sub_180280810`): a weaker /
//!   shorter one never lowers or truncates a stronger / longer, and they do
//!   not stack. Its value is never scaled by the damage-taken multiplier
//!   (`TakeAttack` passes `includeType61 = false` to `ScaleDamage`). It adds
//!   its value to the damage taken by every later hit
//!   (`gd_sim::actor::defender`, `damage_taken_pct`) and lights the HUD's
//!   sundered overlay (`GetFixedDamage(61)`); the tick deals no damage.
//! - Crowd control (`CombatAttributeReflexDamage`): a re-application keeps
//!   `max(remaining, new)` — the fixed timeline only grows and every covered
//!   slot keeps `fmaxf` (`AddFixedDamage` @0x18027a270 → `sub_180280810`),
//!   so a short roll never truncates a long control, and it never
//!   stacks. One involuntary effect at a time, by priority immobilize >
//!   petrify > freeze > trap > sleep > stun > knockdown > take-hit, then confusion >
//!   fear while none of those runs (`UpdateFxAndInfluence` @0x18027b8a0).
//!   Stun, sleep, knockdown, freeze, petrify and immobilize stop
//!   all actions (freeze/petrify also stop the pose); trap stops movement
//!   only (`ControllerMonsterStateTrapped::EnemyFound` still picks
//!   and uses a skill, `ControllerMonsterStateImmobile` keeps the no-op
//!   `EnemyFound` and `ImmobilizeAction::Execute` stops the current skill).
//!   A knockdown lays the body down and throws it (`StartInvoluntaryEffect`
//!   @0x18006f260 → `BeginKnockdown`, [`crate::ragdoll::KnockedDown`]); its end
//!   gets it up. A take-hit (type 49, `BeginTakeHit` → `TakeHitAction::Execute`
//!   @0x1800864d0 → `Character::ExecuteTakeHit` @0x18005d650) plays the flinch
//!   once (`CharState::TakeHit`, animation type 46; no stun: the clip is the
//!   action that blocks the next); no stock record applies one.
//! - Sleep wake: `DurationDamageManager::BeginNewAttack`
//!   @0x18027a1e0 (called at `TakeAttack`'s start, before the hit roll):
//!   every attack style other than retaliation (3) / debuff (4) /
//!   reflection (6) removes the type-43 entries and runs their End — any
//!   melee / ranged / direct attack wakes a sleeper, a miss too
//!   ([`Afflictions::receive`]).
//! - Fear / confusion: `UpdateFxAndInfluence` exerts them through
//!   the controller only while no hard crowd control runs —
//!   `Monster::CombatExertInfluenceConfusion` @0x18037b2e0 →
//!   `ControllerMonster::Confuse` @0x180135970 → the Confused state
//!   (`WalkRandomly` @0x18014a1c0: a random point 2 r..7 m from home;
//!   `OnUpdate` @0x180149f70: every `rand() % 2000 + 1000` ms a random
//!   attack, `UseSkillOnPoint` at a point 2 r..5 m from home), and
//!   `Character::CombatExertInfluenceFear` @0x180068ee0 →
//!   `ControllerMonster::ScareMe` @0x180135cb0 → the Scared state
//!   (`PickRunToPoint` @0x18014b090: `max(fleeDistance, 5)` m away from the
//!   last attacker, ±1 m jitter, re-picked every 1–2 s and on the path's
//!   end). [`crate::combat::cc_states`] drives both for monsters after `CombatSet`
//!   (the `Stunned` component keeps the normal AI off them). The player's
//!   controller ignores both (its `ScareMe` / `Confuse` vtable slots are
//!   no-ops); PORT: Pet-class pets (no `MonsterAi`) keep their own
//!   AI, and a confused monster's random attack is a plain swing at a random
//!   nearby character of any side (the engine uses its best skill at a
//!   random point).
//! - Speed debuffs scale the run/attack/cast animations (and so root-motion
//!   movement): `TotalSpeed` + the specific one.
//!   `Character::GetRunSpeed` / `GetAttackSpeed` / `GetSpellCastSpeed`:
//!   (1 - slow %) is multiplied into the uncapped speed before the
//!   `Cap*Speed` clamp, so the floor is `*SpeedCapMin` % of the bio base
//!   speed (players 20, monsters 20/30/40 by difficulty) and a speed over
//!   its max cap absorbs part of the slow (`gd_sim::actor::SpeedCap`). The
//!   animation factor is that slowed capped speed / the unslowed one.
//! - Effects: `gameengine.dbr` `<type>DamageFxPak` character FX while active.
//! - DoT combat numbers: `DurationDamageManager::Update` @0x180279170
//!   adds each update's damage from the main player's own DoTs
//!   (`ExecuteDamage`: entries whose attacker id, `TakeAttack`'s
//!   `ParametersCombat` +8, is the main player's; not its pets') and the
//!   largest multiplier of all ticked entries; every 1000 ms (a per-character
//!   clock from spawn) it sends the sum when it is ≥ 1, then zeroes both
//!   ([`DotNumbers`] → `crate::floating`).
//! - Kill credit: every DoT entry keeps its attacker (the hit's attacker; a reflected / retaliated hit's DoTs
//!   the reflecting defender). `DurationDamageManager::ExecuteDamage` @0x1802796b0: before each
//!   entry's `CombatManager::ApplyDamage` (types other than 21, mana leech) the defender's `CombatManager`
//!   attacker (+364, `Character` +1692) is set to the entry's attacker; `ApplyDamage` @0x18011d340 registers the
//!   damage for it (`GameEngine::RegisterDamage`) and, on the death, runs that attacker's `OnEnemyDeath` with
//!   the entry's skills; `Character::ImDying` → `CharacterIsDying` @0x180068db0 sends the creature-death event
//!   with +1692 as the killer (`TakeAttack` @0x18011d950 sets it too, early, for every hit). So a DoT kill is
//!   the DoT's applier's kill and scores / pays / procs as his (`crate::combat::KillCredit`); the killer is the
//!   last entry that dealt damage in the killing update (life leech included). The entry keeps its hit's skill (PORT: a
//!   same-source stack keeps the stronger application's), so the kill reports it
//!   (`ActivateOnEnemyDeathSecondarySkills`); a reflected hit's DoT has none. PORT: the port's entry
//!   order (`gd_sim::combat::DotTick::last_source`) and leech-before-DoT order differ from the engine's list.
//! - Stun sound: `UpdateFxAndInfluence` calls the old effect's
//!   `End*` and the new one's `Begin*` only when the effect changes (+40);
//!   `Character::BeginStun` / `BeginSleep` → the controller's `BeginStun` /
//!   `BeginSleep` → the `Stunned` / `Sleep` state (`ControllerPlayerState::
//!   DefaultBeginStunAction`, `ControllerMonsterState::BeginStun`), whose
//!   `OnBegin` makes a `TakeStunAction` / `TakeSleepAction`; its `Execute`
//!   @0x180086180 / @0x180086660 on a living (`IsAlive`) character stops
//!   its skill, runs `ExecuteStun` / `ExecuteSleep` and `Character::
//!   PlayStunSound` @0x18005c9d0 (`crate::combat_sounds::StunSound`). So
//!   once per entry into the stun or sleep state, not on a refresh (also
//!   again after a higher one, e.g. a trap, ends on a running stun).
//!   `TakeTrapAction::Execute` and `BeginFreeze` play no sound (freeze
//!   creates the `freezeDamageHandler`). PORT: every controller state is
//!   taken to switch to `Stunned` (some, e.g. `UseSkill`, override
//!   `BeginStun`; not decompiled).
//! - Sickness (`Skill_BuffSelfSick`, the Burr witch brew; Game.dll):
//!   a `Skill_BuffSelfDuration` (its stats for `skillActiveDuration`;
//!   `crate::casting::Kind::Buff`) whose `Load` @0x1804eb3d0 reads
//!   `stunDuration` / `stunPeriod` (s → whole ms) and `stunChance` (an int,
//!   × 0.01); `ActivateNow` @0x1804eb470 zeroes its timer (+401); `Update`
//!   @0x1804eb4a0 while active: timer -= frame ms, at ≤ 0 a `rand()` /
//!   32767 < chance stuns a player holder (`ControllerAI::StunMe`, only for a
//!   `ControllerPlayer`: `ClearTemporaryStates`, the `Stunned` state for
//!   `stunDuration` ms), timer = `stunPeriod`. So the first roll comes with
//!   the first update ([`Sickness`], [`sick_stuns`]). PORT: the stun is the
//!   afflictions' stun entry ([`Afflictions::hold`], its state and sound);
//!   the roll is the port's `crate::rng`.
//! - Icy look ([`Icy`], drawn by grimdawn `icy.rs`): `Character::BeginFreeze` @0x18006f690 /
//!   `BeginPetrify` @0x18006f850 make the `gameengine.dbr` `freezeDamageHandler` / `petrifyDamageHandler`
//!   (`SpecialCharHandler::CreateHandler` @0x180562e30, `handlerType` Freeze → `SpecialCharHandler_IcyCharacter`)
//!   and `Enable` it; `EndFreeze` / `EndPetrify` `Disable` it. `Start` @0x1805634b0 plays `freezeSound` at the
//!   character and gives its mesh and every attached item the `icyEffectName` texture with a `GMIEffectIcey`;
//!   `Disable` @0x180563f90 → `End` @0x180563940 plays `shatterSound` when the character is dead
//!   (`IsAlive`), else `thawSound`, and takes the look off. On a death the look stays 250 ms
//!   (`Character::ImDying` arms +4475 with 250 ms; the dead `UpdateSelf` counts it down and runs
//!   `ClearDamageFxs` @0x18005c350, which disables both handlers): it then shatters ([`shatter_icy`]).
//!   The handler's `iceCubeName` (a `GMIActor_Icy`) is loaded but never spawned: its actor list (+240) is
//!   only read by `End`, nothing fills it (no other code touches it), so no chunk shows in the game.
//!   PORT: `Disable`'s second shatter condition (every entry of Character +9912, not decompiled) is
//!   taken as true; the death timer counts from the first frame the port sees `Dead`.
//! - Death ([`clear_on_death`]): `Character::ImDying` @0x18005c0a0 → `DurationDamageManager::ImDead`
//!   @0x18027b7b0: both entry lists are emptied (+72 the damage over time, +96 the fixed entries: debuffs and
//!   crowd control), the involuntary effect (+40) is zeroed without its `End*` call (so the icy handler stays
//!   for the 250 ms above) and every running damage effect is stopped (`Character::StopDamageEffect`
//!   @0x18006e8a0 per entry of the +48 map, which is then cleared). So nothing a character suffered outlives
//!   its death; `Character::StartRespawn` @0x18005af90 (teleport, the handlers' `Reset`, visible, life state
//!   5, the spawn effect) has nothing left to clear. A [`tick`] never touches a character whose life is
//!   already 0 (`DurationDamageManager::Update` runs for the living only; a script kill in the same frame
//!   has not marked it `Dead` yet).
use crate::character::{CharState, Character};
use crate::combat::{Combatant, Dead, MonsterKilled, kill};
use crate::data::GameData;
use crate::monsters::{Monster, ProxyStates};
use crate::player::Player;
use bevy::prelude::*;
use gd_sim::actor::{Debuffs, Hit, is_dot_damage};
use gd_sim::attributes::CombatType as T;
use gd_sim::combat::{AttackKind, DotManager};

pub struct AfflictionsPlugin;

impl Plugin for AfflictionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DotNumbers>().add_systems(
            Update,
            (
                add_afflictions,
                sick_stuns,
                tick.in_set(AfflictionStage::Tick),
                clear_on_death,
                shatter_icy,
            )
                .chain()
                .after(crate::combat::CombatSet),
        );
    }
}

pub use crate::movement::{Rooted, Stunned};

#[derive(Component, Default)]
pub struct Afflictions {
    dots: DotManager,
    /// (type, value, seconds left).
    fixed: Vec<(T, f32, f32)>,
    /// (type, seconds left).
    cc: Vec<(T, f32)>,
    /// `DurationDamageManager` +40: the involuntary effect it last switched
    /// to (`UpdateFxAndInfluence`'s End / Begin calls run on a change).
    state: Option<T>,
    /// Attached FX per type.
    fx: Vec<(T, Entity)>,
    /// Life / mana leech over time (`offensiveSlowLifeLeach` /
    /// `offensiveSlowManaLeach`): (type, per second, seconds left, source).
    leech: Vec<(T, f32, f32, Entity)>,
    /// `DurationDamageManager` +120 / +124 / +128: the text clock (ms), the
    /// player's DoT damage and the largest multiplier since it last ran out.
    text: (i32, f32, f32),
    /// Damage applied once without an attacker (`CombatManager::ApplyDamage` straight, no `TakeAttack`: no
    /// resistances), taken at the next [`tick`] through the same filters as a DoT tick: (type, amount).
    direct: Vec<(T, f32)>,
}

impl Afflictions {
    /// `CombatManager::ApplyDamage` of `amount` of `ty` with no attacker ([`Self::direct`]).
    pub fn apply_damage(&mut self, ty: T, amount: f32) {
        self.direct.push((ty, amount));
    }
}

/// One second's DoT damage of the main player on `target` (module docs).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct DotNumbers {
    pub target: Entity,
    pub damage: f32,
    pub multiplier: f32,
}

/// The dressing request of a frozen / petrified character's icy look (module docs, "Icy look"): on the
/// character while its freeze / petrify handler is enabled. grimdawn draws it from [`icy_texture`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Icy {
    /// The petrify handler's look (stone), else the freeze handler's (ice).
    pub petrify: bool,
    /// Milliseconds since the death (`ClearDamageFxs` at [`ICY_DEATH_MS`]).
    dead_ms: i32,
}

impl Icy {
    pub fn new(petrify: bool) -> Self {
        Self {
            petrify,
            dead_ms: 0,
        }
    }
}

/// `Character::Character` (+4474 = 250): how long a dead character keeps its damage effects.
pub const ICY_DEATH_MS: i32 = 250;

/// The handler record of the freeze / petrify look (`BeginFreeze` / `BeginPetrify` defaults).
pub fn icy_handler(data: &GameData, petrify: bool) -> Option<gd_formats::arz::Record> {
    let engine = data.engine();
    data.db.get_owned(if petrify {
        &engine.petrify_handler
    } else {
        &engine.freeze_handler
    })
}

/// The look's texture, `icyEffectName` (`SpecialCharHandler_IcyCharacter::LoadFromTable`
/// @0x180563e20, default `xpack/effects/textures/icy.tex`).
pub fn icy_texture(data: &GameData, petrify: bool) -> String {
    icy_handler(data, petrify)
        .and_then(|r| r.str("icyEffectName").map(str::to_owned))
        .unwrap_or_else(|| "xpack/effects/textures/icy.tex".to_owned())
}

/// Plays the handler's `freezeSound` / `thawSound` / `shatterSound` at `at` (`SoundPak::Play3D`).
fn icy_sound(commands: &mut Commands, data: &GameData, petrify: bool, field: &str, at: Vec3) {
    if let Some(pak) = icy_handler(data, petrify).and_then(|r| r.str(field).map(str::to_owned)) {
        commands.write_message(crate::visuals::PlaySound { pak, at: Some(at) });
    }
}

/// The involuntary effect's handler: `Some(petrify)` for freeze / petrify.
fn icy_kind(cc: Option<T>) -> Option<bool> {
    match cc {
        Some(T::Freeze) => Some(false),
        Some(T::Petrify) => Some(true),
        _ => None,
    }
}

/// A character that died loses every DoT, debuff and crowd control (module docs, "Death").
fn clear_on_death(mut commands: Commands, mut q: Query<(Entity, &mut Afflictions), Added<Dead>>) {
    for (e, mut a) in &mut q {
        for fx in a.died() {
            commands.entity(fx).try_despawn();
        }
        commands
            .entity(e)
            .try_remove::<(Stunned, Rooted, crate::ragdoll::KnockedDown)>();
    }
}

/// A dead character's icy look shatters [`ICY_DEATH_MS`] after the death (module docs).
fn shatter_icy(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<(Entity, &mut Icy, &Transform), With<Dead>>,
    mut carry: Local<f32>,
) {
    let ms = crate::session::frame_ms(&mut carry, &time);
    for (e, mut icy, tf) in &mut q {
        icy.bypass_change_detection().dead_ms += ms;
        if icy.dead_ms >= ICY_DEATH_MS {
            icy_sound(
                &mut commands,
                &data,
                icy.petrify,
                "shatterSound",
                tf.translation,
            );
            commands.entity(e).try_remove::<Icy>();
        }
    }
}

/// `DurationDamageManager::UpdateFxAndInfluence` @0x18027b8a0: the
/// hard crowd control first (immobilize > petrify > freeze > trap > sleep >
/// stun > knockdown > take-hit: 47, 46, 45, 44, 43, 42, 48, 49), then — only
/// while none of those is active — confusion (53) before fear (52), the
/// controller influences ([`crate::combat::cc_states`]).
const CC_PRIORITY: [T; 10] = [
    T::Immobilize,
    T::Petrify,
    T::Freeze,
    T::Trap,
    T::Sleep,
    T::Stun,
    T::Knockdown,
    T::TakeHit,
    T::Confusion,
    T::Fear,
];

impl Afflictions {
    /// A damage over time of `dps` for `secs` applied by `source` (who gets
    /// the kill, module docs); a synthetic DoT (the `GD_DEBUFFS` hook) keys
    /// its slot by the source entity too.
    pub fn add_dot(&mut self, ty: T, dps: f32, secs: f32, source: Entity) {
        self.dots
            .add(ty, dps, secs, source.to_bits(), source.to_bits());
    }

    /// Dispel: all damage over time ends (`dispelDamageOverTime`).
    pub fn clear_dots(&mut self) {
        self.dots = DotManager::default();
        self.leech.clear();
    }

    /// The character died (module docs, "Death"): every DoT, leech, debuff and crowd control ends; returns
    /// the attached effects to remove. The icy look is left to [`shatter_icy`].
    fn died(&mut self) -> Vec<Entity> {
        self.clear_dots();
        self.fixed.clear();
        self.cc.clear();
        self.state = None;
        self.fx.drain(..).map(|f| f.1).collect()
    }

    /// No DoT, leech, debuff, crowd control, involuntary state, effect or pending direct damage.
    fn is_idle(&self) -> bool {
        self.dots.types().next().is_none()
            && self.leech.is_empty()
            && self.fixed.is_empty()
            && self.cc.is_empty()
            && self.state.is_none()
            && self.fx.is_empty()
            && self.direct.is_empty()
    }

    /// A fixed crowd-control entry `ty` for `secs` (`AddFixedDamage`: the
    /// duration only grows, [`Self::refresh_cc`]): a trap / freeze debuff's
    /// hold (`SkillBuff_DebufTrap::ApplyBuffDamage`, `crate::skill_buffs`),
    /// Blade Barrier's refresh (`crate::casting::ImmobilizeRefresh`).
    pub(crate) fn hold(&mut self, ty: T, secs: f32) {
        self.refresh_cc(ty, secs);
    }

    /// `DurationDamageManager::AddFixedDamage` @0x18027a270 → the
    /// fixed slot adder `sub_180280810`: the fixed timeline only grows
    /// (`slot_count < (int)(duration * 10)` extends it, never shortens) and
    /// every covered slot keeps `fmaxf(old, new)`. So a re-application keeps
    /// `max(remaining, new)` — a shorter roll never truncates a longer
    /// control — and never stacks. `ModifyDuration` @0x18027b6d0 scales
    /// types 42..49 only when `FactionManager::GetPvpIds` holds (a joined
    /// PvP game): by design not here (no multiplayer / PvP).
    fn refresh_cc(&mut self, ty: T, secs: f32) {
        if secs <= 0.0 {
            return;
        }
        match self.cc.iter_mut().find(|c| c.0 == ty) {
            Some(c) => c.1 = c.1.max(secs),
            None => self.cc.push((ty, secs)),
        }
    }

    pub fn debuffs(&self) -> Debuffs {
        Debuffs(self.fixed.iter().map(|f| (f.0, f.1)).collect())
    }

    /// In the Sleep state: the character's action state is 11 (`Character::ExecuteSleep`, run by the
    /// `TakeSleepAction` the controller's Sleep state makes, module docs), which the monsters'
    /// `ignoreSleepingEnemies` reads (`combat::ai`).
    pub fn asleep(&self) -> bool {
        self.state == Some(T::Sleep)
    }

    /// The active involuntary effect ([`CC_PRIORITY`]): the hard crowd
    /// control, or confusion / fear while none of those runs
    /// (`UpdateFxAndInfluence`). [`crate::combat::cc_states`] drives the latter two.
    pub fn cc_state(&self) -> Option<T> {
        self.involuntary()
    }

    /// The type-61 duration-damage value, which lights the HUD's `sundered`
    /// overlay (`Character::GetDamageMultiplier` @0x180069510 =
    /// `DurationDamageManager::GetFixedDamage(61)`, the HUD tests it `!= 0`):
    /// the largest active `offensiveSlowDamageMult*` entry (the fixed slots
    /// keep `fmaxf`, module doc), 0 without one.
    pub fn damage_multiplier(&self) -> f32 {
        self.fixed(T::DamageMultiplier)
    }

    /// The DoTs, debuffs and crowd control a hit carries.
    pub fn receive(&mut self, hit: &Hit, source: Entity, skill: Option<&str>) {
        // `DurationDamageManager::BeginNewAttack` @0x18027a1e0 (called
        // at `TakeAttack`'s start, before the hit roll): every attack style
        // other than retaliation (3), debuff (4) and reflection (6) removes
        // the type-43 (Sleep) entries and runs their End — any melee / ranged
        // / direct attack wakes a sleeper, a miss too.
        if !matches!(
            hit.kind,
            AttackKind::Retaliation | AttackKind::Debuff(_) | AttackKind::Reflection
        ) {
            self.cc.retain(|c| c.0 != T::Sleep);
        }
        for d in &hit.dots {
            if d.dps[0] <= 0.0 || d.duration <= 0.0 {
                continue;
            }
            if is_dot_damage(d.ty) {
                // The slot key is the DoT's owning source (item instance /
                // skill record / passive, `gd_sim::actor::dot_source_key`),
                // not the attacker: distinct sources stack, the same source's
                // re-application keeps max
                // (`DamageAttributeDur::AddDamageToAccumulator` @0x180190560).
                // The attacker rides along for the kill credit (module docs).
                self.dots.add_hit(
                    d.ty,
                    d.dps[0],
                    d.duration,
                    d.source,
                    source.to_bits(),
                    d.hit_multiplier,
                    skill,
                );
            } else if matches!(d.ty, T::LifeLeech | T::ManaLeech) {
                self.leech.push((d.ty, d.dps[0], d.duration, source));
            } else {
                // Fixed entries (debuffs and type 61 alike): each application
                // is its own entry; the active value is the largest
                // (`Debuffs::get`, `damage_multiplier`) and a shorter / weaker
                // one never truncates a longer / stronger (the fixed slots'
                // `fmaxf`, `sub_180280810`). Type 61's value is never scaled
                // by the damage-taken multiplier
                // (`CombatManager::TakeAttack` @0x18011d950:
                // `ScaleDamage(.., includeType61 = false, ..)` @0x18011af00
                // skips type 61; `gd_sim::combat::Damage::scale`).
                // `ModifyDuration` leaves type 61 alone (only 42..49, in PvP,
                // see `refresh_cc`).
                self.fixed.push((d.ty, d.dps[0], d.duration));
            }
        }
        for &(ty, secs) in &hit.reflex {
            self.refresh_cc(ty, secs);
        }
    }

    /// [`Self::status`] with each type's magnitude for the buff bar's tooltip (`sub_14024F890`: a DoT's damage per
    /// second, a debuff's value; 0 for crowd control).
    pub fn status_detail(&self) -> Vec<(T, f32, i32)> {
        self.status()
            .into_iter()
            .map(|(t, secs)| {
                let dot = self.dots.per_second(t);
                let m = if dot > 0.0 {
                    dot
                } else {
                    self.fixed
                        .iter()
                        .filter(|f| f.0 == t)
                        .map(|f| f.1.abs())
                        .fold(0.0, f32::max)
                };
                (t, secs, m.round() as i32)
            })
            .collect()
    }

    /// Active types with seconds left (HUD icons): DoTs, debuffs, crowd
    /// control, in the engine's strip order (see the body).
    pub fn status(&self) -> Vec<(T, f32)> {
        let mut v: Vec<(T, f32)> = Vec::new();
        let mut put = |t: T, secs: f32| match v.iter_mut().find(|x| x.0 == t) {
            Some(x) => x.1 = x.1.max(secs),
            None => v.push((t, secs)),
        };
        // `DurationDamageManager::GetDamageData` @0x18027ab50: the
        // normal duration-damage list in insertion order, then the fixed-damage
        // list (never sorted by type or time).
        for t in self.dots.types() {
            let r = self.dots.remaining(t);
            if r > 0.0 {
                put(t, r);
            }
        }
        for f in &self.fixed {
            put(f.0, f.2);
        }
        for c in &self.cc {
            put(c.0, c.1);
        }
        v
    }

    fn involuntary(&self) -> Option<T> {
        CC_PRIORITY
            .iter()
            .copied()
            .find(|t| self.cc.iter().any(|c| c.0 == *t && c.1 > 0.0))
    }

    fn fixed(&self, ty: T) -> f32 {
        self.fixed
            .iter()
            .filter(|f| f.0 == ty)
            .map(|f| f.1)
            .fold(0.0, f32::max)
    }

    /// Types with a visible effect right now.
    fn active_types(&self) -> Vec<T> {
        let mut v: Vec<T> = [
            T::Fire,
            T::Cold,
            T::Lightning,
            T::Acid,
            T::Vitality,
            T::Bleeding,
            T::Aether,
            T::Chaos,
        ]
        .into_iter()
        .filter(|t| self.dots.is_active(*t))
        .collect();
        if self.fixed(T::RunSpeed) > 0.0 || self.fixed(T::TotalSpeed) > 0.0 {
            v.push(T::RunSpeed);
        }
        if let Some(c) = self.involuntary() {
            v.push(c);
        }
        v
    }
}

/// `gameengine.dbr` character FX pak field per type.
fn fx_field(ty: T) -> Option<&'static str> {
    Some(match ty {
        T::Fire => "fireDamageFxPak",
        T::Cold => "coldDamageFxPak",
        T::Lightning => "lightningDamageFxPak",
        T::Acid => "poisonDamageFxPak",
        T::Vitality | T::Chaos => "lifeDamageFxPak",
        T::Bleeding => "bleedingDamageFxPak",
        T::Aether => "aetherDamageFxPak",
        T::RunSpeed => "runSpeedDamageFxPak",
        T::Stun | T::Knockdown | T::Sleep => "stunDamageFxPak",
        T::Trap | T::Immobilize => "trapDamageFxPak",
        _ => return None,
    })
}

fn add_afflictions(
    hooks: Res<crate::hooks::Hooks>,
    mut commands: Commands,
    q: Query<(Entity, Has<Player>), (With<Combatant>, Without<Afflictions>)>,
) {
    for (e, is_player) in &q {
        let mut a = Afflictions::default();
        // `GD_DEBUFFS` (hooks.rs).
        if is_player && hooks.gameplay.debuffs {
            a.add_dot(T::Fire, 5.0, 40.0, Entity::PLACEHOLDER);
            a.add_dot(T::Bleeding, 5.0, 25.0, Entity::PLACEHOLDER);
            a.fixed.push((T::RunSpeed, 30.0, 35.0));
            a.fixed.push((T::DefensiveReduction, 50.0, 30.0));
            a.fixed.push((T::OffensiveAbility, 50.0, 20.0));
            a.cc.push((T::Stun, 3.0));
        }
        commands.entity(e).try_insert(a);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &Transform,
            &mut Afflictions,
            &mut Combatant,
            &mut Character,
            Option<&Monster>,
            Has<Player>,
            Has<Stunned>,
            Has<Rooted>,
            Option<&mut crate::casting::Caster>,
            Option<&mut crate::casting::Shields>,
            Has<Icy>,
            &mut crate::combat::Attackers,
            Has<crate::combat::MonsterAi>,
        ),
        Without<Dead>,
    >,
    mut states: ResMut<ProxyStates>,
    mut killed: MessageWriter<MonsterKilled>,
    mut carry: Local<f32>,
    (players, mut numbers, invincible, links, pets, dummies): (
        Query<Entity, With<Player>>,
        MessageWriter<DotNumbers>,
        Query<(), With<crate::combat::Invincible>>,
        Query<&crate::reflect_link::ReflectLinks>,
        Query<&crate::pets::Pet>,
        Query<(), With<crate::world::Dummy>>,
    ),
) {
    let player = players.single().ok().map(Entity::to_bits);
    let dt = time.delta_secs();
    let ms = crate::session::frame_ms(&mut carry, &time);
    // Read only when an effect is attached.
    let mut engine: Option<std::sync::Arc<crate::rules::GameEngineRules>> = None;
    // Leeched life / energy for the sources: (source, type, amount).
    let mut credits: Vec<(Entity, T, f32)> = Vec::new();
    for (
        e,
        tf,
        mut a,
        mut cb,
        mut ch,
        monster,
        is_player,
        stunned,
        rooted,
        mut caster,
        mut shield,
        icy,
        mut attackers,
        is_monster,
    ) in &mut q
    {
        // Already killed this frame (a script kill sets the life before `Dead` lands): module docs, "Death".
        if cb.life <= 0.0 {
            continue;
        }
        // Nothing active: only the text clock runs (no `&mut` deref, so `Afflictions` stays unchanged).
        if a.is_idle() && !stunned && !rooted && !ch.frozen && ch.slow == [1.0; 3] {
            let a = a.bypass_change_detection();
            // Keeps the DoT timeline's sub-tick phase (no timelines: nothing else happens).
            a.dots.update_from(ms, None);
            let t = &mut a.text;
            t.0 += ms;
            if t.0 >= 1000 {
                if t.1 >= 1.0 {
                    numbers.write(DotNumbers {
                        target: e,
                        damage: t.1,
                        multiplier: t.2,
                    });
                }
                *t = (0, 0.0, 0.0);
            }
            continue;
        }
        let a = &mut *a;
        let mut lost = 0.0;
        // The killer if this update kills (module docs, "Kill credit").
        let mut last: Option<Entity> = None;
        let mut last_skill: Option<String> = None;
        // What each source took this update (`ApplyDamage` → `RegisterDamage`, [`crate::combat::Attackers`]).
        let mut took: Vec<(Entity, f32)> = Vec::new();
        // Leech over time: drained from this character, given to the source
        // 1:1, the gain capped at the life / energy the target has left
        // (`DurationDamageManager::ExecuteDamage` types 20/21;
        // characters without energy give none); per frame here.
        for l in a.leech.iter_mut() {
            let secs = dt.min(l.2);
            l.2 -= dt;
            let amount = l.1 * secs;
            if amount > 0.0 && l.0 == T::LifeLeech {
                last = Some(l.3);
            }
            match l.0 {
                T::LifeLeech => {
                    let take = amount.min((cb.life - lost).max(0.0));
                    lost += amount;
                    credits.push((l.3, T::LifeLeech, take));
                    took.push((l.3, amount));
                }
                _ => {
                    if let Some(c) = caster.as_mut() {
                        let take = amount.min(c.energy.max(0.0));
                        c.energy -= take;
                        credits.push((l.3, T::ManaLeech, take));
                    }
                }
            }
        }
        a.leech.retain(|l| l.2 > 0.0);
        // DoT ticks go through the damage-absorbing buffs too
        // (`DurationDamageManager::ExecuteDamage` → `CombatManager::
        // ApplyDamage`).
        let dot = a.dots.update_from(ms, player);
        if let Some(src) = dot.last_source.and_then(Entity::try_from_bits) {
            last = Some(src);
            last_skill = dot.last_skill.as_deref().map(str::to_owned);
        }
        a.text.1 += dot.from_source;
        a.text.2 = a.text.2.max(dot.multiplier);
        a.text.0 += ms;
        if a.text.0 >= 1000 {
            if a.text.1 >= 1.0 {
                numbers.write(DotNumbers {
                    target: e,
                    damage: a.text.1,
                    multiplier: a.text.2,
                });
            }
            a.text = (0, 0.0, 0.0);
        }
        let (raw, before) = (dot.per_type.iter().map(|t| t.1).sum::<f32>(), lost);
        let direct = std::mem::take(&mut a.direct);
        let mut direct_lost = 0.0;
        let dot_types = dot.per_type.len();
        for (i, (ty, v)) in dot.per_type.into_iter().chain(direct).enumerate() {
            let v = match shield.as_deref_mut() {
                Some(sh) if !sh.0.is_empty() => sh.filter(ty, v),
                _ => v,
            };
            // A reflect link's filter cuts the tick too (`ApplyDamage` runs
            // every filter; `ModifyDamage` @0x1804d3d40); a DoT is
            // not passed on (only `TakeAttack` does that).
            let v = links.get(e).map_or(v, |l| {
                l.0.iter().fold(v, |v, l| {
                    if l.absorb > 0.0 {
                        v - v * l.absorb * 0.01
                    } else {
                        v
                    }
                })
            });
            if i < dot_types {
                lost += v;
            } else {
                direct_lost += v;
            }
            debug!("{e}: {ty:?} over time {v:.1}");
        }
        if let Some(sh) = shield.as_mut()
            && sh.0.iter().any(|s| s.absorb.spent())
        {
            sh.drop_spent();
        }
        // PORT: each source's part of the filtered ticks in proportion (the engine filters entry by entry).
        let kept = if raw > 0.0 {
            (lost - before) / raw
        } else {
            0.0
        };
        took.extend(
            dot.by_source
                .iter()
                .filter_map(|(s, d)| Entity::try_from_bits(*s).map(|s| (s, d * kept))),
        );
        // Direct damage has no source to credit.
        lost += direct_lost;
        a.fixed.iter_mut().for_each(|f| f.2 -= dt);
        a.fixed.retain(|f| f.2 > 0.0);
        a.cc.iter_mut().for_each(|c| c.1 -= dt);
        a.cc.retain(|c| c.1 > 0.0);

        // Slows, folded in before the speed caps
        // (`Character::GetRunSpeed` / `GetAttackSpeed` / `GetSpellCastSpeed`).
        let total = a.fixed(T::TotalSpeed);
        let caps = &cb.profile.speed_caps;
        let k = |i: usize, x: f32| {
            let unslowed = caps[i].capped(0.0);
            if total + x <= 0.0 || unslowed <= 0.0 {
                1.0
            } else {
                (caps[i].capped(total + x) / unslowed).min(1.0)
            }
        };
        let slow = [
            k(0, a.fixed(T::RunSpeed)),
            k(1, a.fixed(T::AttackSpeed)),
            k(2, a.fixed(T::CastSpeed)),
        ];
        if ch.slow != slow {
            ch.slow = slow;
        }
        // Crowd control.
        let cc = a.involuntary();
        // Entering a stun or sleep: the stunned / asleep controller state's
        // `TakeStunAction` / `TakeSleepAction` plays the stun sound (module
        // docs).
        let changed = a.state != cc;
        if changed {
            // The old effect's `End*`, then the new one's `Begin*` (module docs, "Icy look").
            if icy_kind(a.state).is_some() && icy {
                let petrify = icy_kind(a.state) == Some(true);
                icy_sound(&mut commands, &data, petrify, "thawSound", tf.translation);
                commands.entity(e).try_remove::<Icy>();
            }
            if let Some(petrify) = icy_kind(cc) {
                icy_sound(&mut commands, &data, petrify, "freezeSound", tf.translation);
                commands.entity(e).try_insert(Icy::new(petrify));
            }
            // `EndKnockdown`: it gets up once its throw is over (`crate::ragdoll`).
            if a.state == Some(T::Knockdown) {
                commands
                    .entity(e)
                    .try_insert(crate::ragdoll::KnockedDown { up: true });
            }
            // `BeginKnockdown`: `crate::ragdoll` lays it down and throws it.
            if cc == Some(T::Knockdown) {
                commands
                    .entity(e)
                    .try_insert(crate::ragdoll::KnockedDown::default());
            }
            // `ExecuteTakeHit`: the flinch, unless one plays (action state 10).
            if cc == Some(T::TakeHit) && ch.state != CharState::TakeHit {
                ch.state = CharState::TakeHit;
                ch.playing = gd_sim::anim::AnimType::None;
            }
            a.state = cc;
            if matches!(cc, Some(T::Stun | T::Sleep)) {
                commands.write_message(crate::combat_sounds::StunSound { holder: e });
            }
        }
        let (no_act, no_move) = match cc {
            Some(T::Trap) => (false, true),
            // Fear / confusion: the controller takes over (`crate::combat::cc_states`
            // drives the monster after `CombatSet`); `Stunned` keeps the normal
            // AI off it, but it moves (and a confused one attacks). Only
            // monsters: the player's controller ignores both
            // (`ControllerPlayer`'s `ScareMe` / `Confuse` slots are no-ops), and
            // Pet-class pets keep their own AI (PORT: `pets::ai` drives them).
            Some(T::Fear | T::Confusion) => (is_monster, false),
            Some(T::TakeHit) => (false, false),
            Some(_) => (true, true),
            None => (false, false),
        };
        let frozen = matches!(cc, Some(T::Freeze | T::Petrify));
        if ch.frozen != frozen {
            ch.frozen = frozen;
        }
        if no_act != stunned {
            if no_act {
                commands.entity(e).try_insert(Stunned);
                commands.entity(e).try_remove::<crate::casting::Casting>();
            } else {
                commands.entity(e).try_remove::<Stunned>();
                if ch.state == CharState::Stunned {
                    ch.state = CharState::Idle;
                }
            }
        }
        // An action-blocking control as it begins or takes over from another (a stun on a knocked-down body
        // stands it up into the stun loop): the stun loop, except frozen (the pose stops) and knocked down
        // (`crate::ragdoll` lays the body down).
        if no_act && changed && !frozen && cc != Some(T::Knockdown) {
            ch.state = CharState::Stunned;
        }
        if no_move != rooted {
            if no_move {
                commands.entity(e).try_insert(Rooted);
            } else {
                commands.entity(e).try_remove::<Rooted>();
            }
        }
        // Effects.
        let want = a.active_types();
        a.fx.retain(|(t, fx)| {
            let keep = want.contains(t);
            if !keep {
                commands.entity(*fx).try_despawn();
            }
            keep
        });
        for t in want {
            if a.fx.iter().any(|f| f.0 == t) {
                continue;
            }
            let engine = engine.get_or_insert_with(|| data.engine());
            let Some(pak) = fx_field(t)
                .and_then(|f| engine.record.as_ref()?.str(f))
                .and_then(|p| data.db.get(p))
            else {
                continue;
            };
            let Some(name) = pak
                .strs_iter("particleEffectNames")
                .next()
                .map(|s| s.to_string())
            else {
                continue;
            };
            let fx = commands
                .spawn((
                    crate::visuals::EffectSpec::record(name),
                    Transform::from_xyz(0.0, 1.0, 0.0),
                    ChildOf(e),
                ))
                .id();
            a.fx.push((t, fx));
            let sounds = crate::casting::char_pak_sounds(&data, std::slice::from_ref(&pak.name));
            commands.queue(move |w: &mut World| {
                crate::casting::start_char_pak_sounds(w, &sounds, e, &[fx])
            });
        }

        if lost > 0.0 && !invincible.contains(e) {
            for (s, d) in took {
                let master = pets.get(s).map_or(s, |p| p.owner);
                attackers.register(s, master, d, !is_player);
            }
            if last.is_some() {
                attackers.last = last;
            }
            cb.life -= lost;
            // A training dummy never dies (`crate::world::Dummy`).
            if dummies.contains(e) {
                cb.life = cb.life.max(1.0);
            }
            if cb.life <= 0.0 {
                cb.life = 0.0;
                for (_, fx) in a.fx.drain(..) {
                    commands.entity(fx).try_despawn();
                }
                kill(
                    &mut commands,
                    e,
                    tf.translation,
                    Some(&mut ch),
                    monster,
                    is_player,
                    &mut states,
                    &mut killed,
                    last.map(|by| (by, last_skill)),
                );
            }
        }
    }
    for (source, ty, amount) in credits {
        let Ok((_, _, _, mut cb, _, _, _, _, _, caster, _, _, _, _)) = q.get_mut(source) else {
            continue;
        };
        if ty == T::LifeLeech {
            cb.life = (cb.life + amount).min(cb.profile.sheet.life);
        } else if let Some(mut c) = caster {
            c.energy = (c.energy + amount).min(cb.profile.sheet.energy);
        }
    }
}

/// The sick buff class (module docs).
pub const BUFF_SELF_SICK: &str = "Skill_BuffSelfSick";

/// The player's running `Skill_BuffSelfSick` timers (module docs): skill,
/// seconds to the next roll, its time left last frame (a recast restarts
/// it: `ActivateNow`).
#[derive(Component, Default, Debug)]
pub struct Sickness(pub Vec<(String, f32, f32)>);

/// A sick buff's `stunDuration`, `stunPeriod` (s, whole ms) and
/// `stunChance` (0..1).
pub fn sick_profile(rec: &gd_formats::arz::Record) -> (f32, f32, f32) {
    let ms = |k: &str| (rec.f32(k).unwrap_or(0.0) * 1000.0) as i32 as f32 / 1000.0;
    (
        ms("stunDuration"),
        ms("stunPeriod"),
        rec.f32("stunChance").unwrap_or(0.0) as i32 as f32 * 0.01,
    )
}

/// `Skill_BuffSelfSick::Update` for the player's timed buffs (module docs).
fn sick_stuns(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &crate::player::Loadout,
            &mut Afflictions,
            Option<&mut Sickness>,
        ),
        (With<Player>, Without<Dead>),
    >,
) {
    let dt = time.delta_secs();
    for (e, loadout, mut aff, sick) in &mut q {
        let running: Vec<(&str, f32, &gd_formats::arz::Record)> = loadout
            .buffs
            .list
            .iter()
            .filter(|b| b.2 > 0.0)
            .filter_map(|b| {
                data.db
                    .get(&b.0)
                    .filter(|r| r.class == BUFF_SELF_SICK)
                    .map(|r| (b.0.as_str(), b.2, r))
            })
            .collect();
        let Some(mut sick) = sick else {
            if !running.is_empty() {
                commands.entity(e).try_insert(Sickness::default());
            }
            continue;
        };
        sick.0.retain(|t| running.iter().any(|r| r.0 == t.0));
        for (skill, left, rec) in running {
            let i = match sick.0.iter().position(|t| t.0 == skill) {
                Some(i) => i,
                None => {
                    sick.0.push((skill.to_owned(), 0.0, left));
                    sick.0.len() - 1
                }
            };
            let t = &mut sick.0[i];
            if left > t.2 {
                t.1 = 0.0;
            }
            t.2 = left;
            t.1 -= dt;
            if t.1 <= 0.0 {
                let (stun, period, chance) = sick_profile(rec);
                if crate::rng::f32() < chance {
                    aff.hold(T::Stun, stun);
                }
                t.1 = period;
            }
        }
    }
}

/// Afflictions tick: what reacts to a status running out orders after `Tick`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AfflictionStage {
    Tick,
}
