//! Damage over time, debuffs and crowd control on characters, after `DurationDamageManager` (Game.dll):
//!
//! - DoTs (`AddDamage`): per damage type a 100 ms timeline, `dps × 0.1` per slot for the duration; the
//!   same source keeps the larger value, different sources stack (`gd_sim::combat::DotManager`). The
//!   source is the owning object + attribute (item instance / skill record / passive), not the attacker
//!   (`DamageAttributeDur::AddDamageToAccumulator`; `gd_sim::actor::Profile::dot_sources`).
//! - Debuffs (`AddFixedDamage`, slows, OA/DA/armor/resistance/damage reductions, fumble): per type every
//!   slot keeps the largest value (`GetFixedDamage`); not scaled by critical hits
//!   (`CombatAttributeDurFixedDamage`).
//! - Type 61 (`offensiveSlowDamageMult*`, `CombatAttributeDur_DamageMultiplier::Execute`): a fixed entry
//!   of value % and duration s; the active value is the largest (`AddFixedSlot`'s `fmaxf`), entries do
//!   not stack. Never scaled by the damage-taken multiplier (`ScaleDamage`'s `includeType61 = false`).
//!   It adds to the damage taken by every later hit (`gd_sim::actor::defender`) and lights the HUD's
//!   sundered overlay; the tick deals no damage.
//! - Crowd control (`CombatAttributeReflexDamage`): a re-application keeps `max(remaining, new)`
//!   (`AddFixedSlot`), never stacks. One involuntary effect at a time, by priority immobilize > petrify >
//!   freeze > trap > sleep > stun > knockdown > take-hit, then confusion > fear while none of those runs
//!   (`UpdateFxAndInfluence`). Stun, sleep, knockdown, freeze, petrify and immobilize stop all actions
//!   (freeze/petrify also stop the pose); trap stops movement only (`ControllerMonsterStateTrapped`
//!   still uses a skill). A knockdown lays the body down and throws it (`BeginKnockdown`,
//!   [`crate::ragdoll::KnockedDown`]). A take-hit (type 49, `Character::ExecuteTakeHit`) plays the
//!   flinch once (animation type 46, no stun); no stock record applies one.
//! - Sleep wake (`DurationDamageManager::BeginNewAttack`, at `TakeAttack`'s start): every attack style
//!   other than retaliation (3) / debuff (4) / reflection (6) removes the type-43 entries, a miss too.
//! - Fear / confusion: exerted through the controller only while no hard crowd control runs —
//!   `ControllerMonster::Confuse` (`WalkRandomly` 2 r..7 m from home; every `rand() % 2000 + 1000` ms a
//!   random attack at a point 2 r..5 m from home) and `ControllerMonster::ScareMe` (`PickRunToPoint`:
//!   `max(fleeDistance, 5)` m from the last attacker, ±1 m jitter, re-picked every 1–2 s and on the
//!   path's end). [`crate::combat::cc_states`] drives both for monsters. The player's controller
//!   ignores both. PORT: Pet-class pets keep their own AI, and a confused monster's random attack is a
//!   plain swing at a random nearby character (the engine uses its best skill at a random point).
//! - Speed debuffs (`TotalSpeed` + the specific one) scale the run/attack/cast animations:
//!   `Character::GetRunSpeed` / `GetAttackSpeed` / `GetSpellCastSpeed` multiply (1 - slow %) into the
//!   uncapped speed before the `Cap*Speed` clamp (floor `*SpeedCapMin` % of the base speed;
//!   `gd_sim::actor::SpeedCap`). The animation factor is the slowed capped speed / the unslowed one.
//! - Effects: `gameengine.dbr` `<type>DamageFxPak` character FX while active.
//! - DoT combat numbers (`DurationDamageManager::Update`): the main player's own DoT damage (not its
//!   pets') and the largest multiplier accumulate; every 1000 ms (per-character clock from spawn) the sum
//!   is sent when ≥ 1, then both zero ([`DotNumbers`] → `crate::floating`).
//! - Kill credit: every DoT entry keeps its attacker (a reflected hit's: the reflecting defender) and
//!   its hit's skill. `DurationDamageManager::ExecuteDamage` sets the defender's attacker to the entry's
//!   before each `CombatManager::ApplyDamage` (types other than 21), so a DoT kill is the applier's
//!   (`crate::combat::KillCredit`) and reports the skill (`ActivateOnEnemyDeathSecondarySkills`).
//!   PORT: a same-source stack keeps the stronger application's skill; entry order
//!   (`gd_sim::combat::DotTick::last_source`) and leech-before-DoT order differ from the engine's list.
//! - Stun sound: `UpdateFxAndInfluence` calls `End*` / `Begin*` only when the effect changes;
//!   `BeginStun` / `BeginSleep` → the controller's `Stunned` / `Sleep` state → `TakeStunAction` /
//!   `TakeSleepAction`, whose `Execute` on a living character plays `Character::PlayStunSound`
//!   (`crate::combat_sounds::StunSound`): once per entry into the state, not on a refresh. Trap and
//!   freeze play none. PORT: every controller state is taken to switch to `Stunned` (not decompiled).
//! - Sickness (`Skill_BuffSelfSick`, the Burr witch brew): a `Skill_BuffSelfDuration` reading
//!   `stunDuration` / `stunPeriod` (s → whole ms) and `stunChance` (int, × 0.01); `ActivateNow` zeroes
//!   its timer; `Update` while active: timer -= frame ms, at ≤ 0 a `rand() / 32767 < chance` stuns a
//!   player holder (`ControllerAI::StunMe`) for `stunDuration`, timer = `stunPeriod` ([`Sickness`],
//!   [`sick_stuns`]). PORT: the stun is the afflictions' stun entry ([`Afflictions::hold`]).
//! - Icy look ([`Icy`], drawn by grimdawn `icy.rs`): `BeginFreeze` / `BeginPetrify` enable the
//!   `freezeDamageHandler` / `petrifyDamageHandler` (`SpecialCharHandler_IcyCharacter`), `EndFreeze` /
//!   `EndPetrify` disable it. `Start` plays `freezeSound` and gives the mesh and attached items the
//!   `icyEffectName` texture; `End` plays `shatterSound` when dead, else `thawSound`. On a death the look
//!   stays 250 ms (`Character.damageFxLinger`), then shatters ([`shatter_icy`]). The `iceCubeName` actor
//!   is never spawned (nothing fills `iceCubes`). PORT: `Disable`'s second shatter condition (not
//!   decompiled) is taken as true; the death timer counts from the first frame the port sees `Dead`.
//! - Death ([`clear_on_death`], `DurationDamageManager::ImDead`): both entry lists are emptied, the
//!   involuntary effect is zeroed without its `End*` call (the icy handler stays the 250 ms) and every
//!   damage effect stops. [`tick`] skips a character whose life is already 0 (a same-frame script kill
//!   has not marked it `Dead` yet).
use crate::combat::{MonsterKilled, kill};
use crate::monsters::ProxyStates;
use bevy::prelude::*;
use gd_game_core::afflictions::{
    AfflictionStage, BUFF_SELF_SICK, DotNumbers, ICY_DEATH_MS, Icy, Sickness, icy_handler,
    icy_kind, sick_profile,
};
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::monsters::Monster;
use gd_game_core::player::Player;
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
                .after(gd_game_core::combat::CombatSet),
        );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            debuffs_hook
                .after(add_afflictions)
                .before(sick_stuns)
                .run_if(|h: Res<gd_game_core::hooks::Hooks>| h.gameplay.debuffs),
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
    /// `DurationDamageManager.involuntaryEffect`: the involuntary effect it last switched to.
    state: Option<T>,
    fx: Vec<(T, Entity)>,
    /// `offensiveSlowLifeLeach` / `offensiveSlowManaLeach`: (type, per second, seconds left, source).
    leech: Vec<(T, f32, f32, Entity)>,
    /// `DurationDamageManager.textClock` / `.textDamage` / `.textMultiplier`.
    text: (i32, f32, f32),
    /// Damage without an attacker (`CombatManager::ApplyDamage`, no resistances), taken at the next
    /// [`tick`] through the DoT filters: (type, amount).
    direct: Vec<(T, f32)>,
}

impl Afflictions {
    /// `CombatManager::ApplyDamage` with no attacker.
    pub fn apply_damage(&mut self, ty: T, amount: f32) {
        self.direct.push((ty, amount));
    }
}

/// `SoundPak::Play3D` of the icy handler's `field`.
fn icy_sound(commands: &mut Commands, data: &GameData, petrify: bool, field: &str, at: Vec3) {
    if let Some(pak) = icy_handler(data, petrify).and_then(|r| r.str(field).map(str::to_owned)) {
        commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
    }
}

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

/// A dead character's icy look shatters [`ICY_DEATH_MS`] after the death.
fn shatter_icy(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<(Entity, &mut Icy, &Transform), With<Dead>>,
    mut carry: Local<f32>,
) {
    let ms = gd_game_core::session::frame_ms(&mut carry, &time);
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

/// `DurationDamageManager::UpdateFxAndInfluence`: hard crowd control first, then confusion before fear.
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
    /// A DoT whose slot is keyed by `source` too (the `GD_DEBUFFS` hook).
    pub fn add_dot(&mut self, ty: T, dps: f32, secs: f32, source: Entity) {
        self.dots
            .add(ty, dps, secs, source.to_bits(), source.to_bits());
    }

    /// `dispelDamageOverTime`.
    pub fn clear_dots(&mut self) {
        self.dots = DotManager::default();
        self.leech.clear();
    }

    /// Ends everything and returns the attached effects to remove; the icy look is left to [`shatter_icy`].
    fn died(&mut self) -> Vec<Entity> {
        self.clear_dots();
        self.fixed.clear();
        self.cc.clear();
        self.state = None;
        self.fx.drain(..).map(|f| f.1).collect()
    }

    fn is_idle(&self) -> bool {
        self.dots.types().next().is_none()
            && self.leech.is_empty()
            && self.fixed.is_empty()
            && self.cc.is_empty()
            && self.state.is_none()
            && self.fx.is_empty()
            && self.direct.is_empty()
    }

    /// A fixed crowd-control entry `ty` for `secs` (a trap / freeze debuff, Blade Barrier's refresh).
    pub(crate) fn hold(&mut self, ty: T, secs: f32) {
        self.refresh_cc(ty, secs);
    }

    /// `DurationDamageManager::AddFixedSlot`: the timeline only grows and every slot keeps `fmaxf`, so a
    /// re-application keeps `max(remaining, new)`. PORT: `ModifyDuration`'s PvP scaling is not ported.
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

    /// Action state 11 (`Character::ExecuteSleep`), read by `ignoreSleepingEnemies`.
    pub fn asleep(&self) -> bool {
        self.state == Some(T::Sleep)
    }

    /// The active involuntary effect ([`CC_PRIORITY`]).
    pub fn cc_state(&self) -> Option<T> {
        self.involuntary()
    }

    /// `Character::GetDamageMultiplier` (`GetFixedDamage(61)`), which lights the HUD's `sundered` overlay.
    pub fn damage_multiplier(&self) -> f32 {
        self.fixed(T::DamageMultiplier)
    }

    pub fn receive(&mut self, hit: &Hit, source: Entity, skill: Option<&str>) {
        // `DurationDamageManager::BeginNewAttack`: any attack but retaliation / debuff / reflection wakes.
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
                // Keyed by the DoT's owning source (`gd_sim::actor::dot_source_key`), not the attacker.
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
                // Fixed entries (debuffs and type 61): each application is its own entry, the largest wins.
                self.fixed.push((d.ty, d.dps[0], d.duration));
            }
        }
        for &(ty, secs) in &hit.reflex {
            self.refresh_cc(ty, secs);
        }
    }

    /// [`Self::status`] with each type's magnitude (`PlayerStatusIcons::AddDotIcon`: DoT per second, debuff
    /// value, 0 for crowd control).
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

    /// Active types with seconds left (HUD icons).
    pub fn status(&self) -> Vec<(T, f32)> {
        let mut v: Vec<(T, f32)> = Vec::new();
        let mut put = |t: T, secs: f32| match v.iter_mut().find(|x| x.0 == t) {
            Some(x) => x.1 = x.1.max(secs),
            None => v.push((t, secs)),
        };
        // `DurationDamageManager::GetDamageData`: DoTs in insertion order, then the fixed list (unsorted).
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
    mut commands: Commands,
    q: Query<Entity, (With<Combatant>, Without<Afflictions>)>,
) {
    for e in &q {
        commands.entity(e).try_insert(Afflictions::default());
    }
}

/// The player's afflictions start with one of each kind.
#[cfg(feature = "dev")]
fn debuffs_hook(mut q: Query<&mut Afflictions, (With<Player>, Added<Afflictions>)>) {
    for mut a in &mut q {
        a.add_dot(T::Fire, 5.0, 40.0, Entity::PLACEHOLDER);
        a.add_dot(T::Bleeding, 5.0, 25.0, Entity::PLACEHOLDER);
        a.fixed.push((T::RunSpeed, 30.0, 35.0));
        a.fixed.push((T::DefensiveReduction, 50.0, 30.0));
        a.fixed.push((T::OffensiveAbility, 50.0, 20.0));
        a.cc.push((T::Stun, 3.0));
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
            Option<&mut gd_game_core::casting::Caster>,
            Option<&mut gd_game_core::casting::Shields>,
            Has<Icy>,
            &mut gd_game_core::combat::Attackers,
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
        Query<&gd_game_core::reflect_link::ReflectLinks>,
        Query<&crate::pets::Pet>,
        Query<(), With<gd_game_core::world::Dummy>>,
    ),
) {
    let player = players.single().ok().map(Entity::to_bits);
    let dt = time.delta_secs();
    let ms = gd_game_core::session::frame_ms(&mut carry, &time);
    let mut engine: Option<std::sync::Arc<gd_game_core::rules::GameEngineRules>> = None;
    // (source, type, amount) of leeched life / energy.
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
        // Already killed this frame (a script kill sets the life before `Dead` lands).
        if cb.life <= 0.0 {
            continue;
        }
        // Nothing active: only the text clock runs, without a `&mut` deref (change detection).
        if a.is_idle() && !stunned && !rooted && !ch.frozen && ch.slow == [1.0; 3] {
            let a = a.bypass_change_detection();
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
        let mut last: Option<Entity> = None;
        let mut last_skill: Option<String> = None;
        // `ApplyDamage` → `RegisterDamage` per source ([`gd_game_core::combat::Attackers`]).
        let mut took: Vec<(Entity, f32)> = Vec::new();
        // Leech over time (`ExecuteDamage` types 20/21): drained 1:1 to the source, capped at what the
        // target has left; an invincible target (`Character::SubtractLife`) gives no life.
        let invincible_now = invincible.contains(e);
        for l in a.leech.iter_mut() {
            let secs = dt.min(l.2);
            l.2 -= dt;
            let amount = l.1 * secs;
            if amount > 0.0 && l.0 == T::LifeLeech {
                last = Some(l.3);
            }
            match l.0 {
                T::LifeLeech => {
                    let take = if invincible_now {
                        0.0
                    } else {
                        amount.min((cb.life - lost).max(0.0))
                    };
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
        // DoT ticks go through the damage-absorbing buffs (`ExecuteDamage` → `CombatManager::ApplyDamage`).
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
            // A reflect link's filter cuts the tick too (`ModifyDamage`); a DoT is not passed on.
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
        lost += direct_lost;
        a.fixed.iter_mut().for_each(|f| f.2 -= dt);
        a.fixed.retain(|f| f.2 > 0.0);
        a.cc.iter_mut().for_each(|c| c.1 -= dt);
        a.cc.retain(|c| c.1 > 0.0);

        // Slows, folded in before the speed caps.
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
        let cc = a.involuntary();
        // Entering a stun or sleep plays the stun sound.
        let changed = a.state != cc;
        if changed {
            // The old effect's `End*`, then the new one's `Begin*`.
            if icy_kind(a.state).is_some() && icy {
                let petrify = icy_kind(a.state) == Some(true);
                icy_sound(&mut commands, &data, petrify, "thawSound", tf.translation);
                commands.entity(e).try_remove::<Icy>();
            }
            if let Some(petrify) = icy_kind(cc) {
                icy_sound(&mut commands, &data, petrify, "freezeSound", tf.translation);
                commands.entity(e).try_insert(Icy::new(petrify));
            }
            // `EndKnockdown`: it gets up once its throw is over.
            if a.state == Some(T::Knockdown) {
                commands
                    .entity(e)
                    .try_insert(crate::ragdoll::KnockedDown { up: true });
            }
            if cc == Some(T::Knockdown) {
                commands
                    .entity(e)
                    .try_insert(crate::ragdoll::KnockedDown::default());
            }
            // `ExecuteTakeHit`: the flinch, unless one plays.
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
            // Fear / confusion: the controller takes over, monsters only (`ControllerPlayer` ignores both;
            // PORT: `pets::ai` drives Pet-class pets).
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
                commands
                    .entity(e)
                    .try_remove::<gd_game_core::casting::Casting>();
            } else {
                commands.entity(e).try_remove::<Stunned>();
                if ch.state == CharState::Stunned {
                    ch.state = CharState::Idle;
                }
            }
        }
        // An action-blocking control as it begins or takes over (a stun stands a knocked-down body up):
        // the stun loop, except frozen and knocked down.
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
                    gd_game_core::visuals::EffectSpec::record(name),
                    Transform::from_xyz(0.0, 1.0, 0.0),
                    ChildOf(e),
                ))
                .id();
            a.fx.push((t, fx));
            let sounds =
                gd_game_core::casting::char_pak_sounds(&data, std::slice::from_ref(&pak.name));
            commands.queue(move |w: &mut World| {
                gd_game_core::casting::start_char_pak_sounds(w, &sounds, e, &[fx])
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
            c.energy = (c.energy + amount).min(cb.profile.sheet.usable_energy());
        }
    }
}

/// `Skill_BuffSelfSick::Update` for the player's timed buffs.
fn sick_stuns(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &gd_game_core::player::Loadout,
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
                if gd_game_core::rng::f32() < chance {
                    aff.hold(T::Stun, stun);
                }
                t.1 = period;
            }
        }
    }
}
