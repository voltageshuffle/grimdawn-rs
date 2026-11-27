//! Autocast skills ("procs"): item skills with an `itemSkillAutoController` (base item,
//! affixes, components and their bonus, augments) and devotion celestial powers bound to a
//! skill (save: the bound skill's `autocast_skill` / `autocast_controller`).
//!
//! A controller (`skillautocastcontroller.tpl`) has `triggerType`, `chanceToRun` %,
//! `triggerParam`, `targetType` (Self / Enemy / EnemyLocation / Ally) and `autoTargetRadius`.
//! When it triggers and the skill is off cooldown (`skillCooldownTime`, with its charges), the
//! skill fires the same frame (`casting::ProcCast`: no animation, no energy).
//!
//! Triggers (`SkillAutoCastController`'s trigger objects):
//! `HitByEnemy` any hit taken (combat styles 3 / 6 excluded), `HitByMelee` style 1,
//! `HitByProjectile` style 2, `HitByCrit` a critical hit taken, `Block` a hit the shield
//! blocked; `OnKill` (`EnemyDeath`, only on the killer) a dead enemy — for a celestial power
//! only when the killing skill is its bound skill; `LowHealth` / `LowMana` fire on dropping
//! below `triggerParam` % and re-arm once back above it; `CastBuff` / `CastDebuf` per character
//! buffed / debuffed (`SkillActivated::ApplyBuffOnTarget`, [`buff_trigger`]).
//!
//! `AttackEnemy(Crit)`: `Character::AttackTarget` calls `AttackingEnemy` for every attack on a
//! target, hit or miss; crit needs a crit multiplier > 1 (`SkillAutoCastController::IsCritical`).
//! A proc granted by a `Weapon*` item (shields and off-hands included) only fires when the
//! attack carries that item among its weapon ids (`SkillAutoCastController::AttackUsesItem`).
//! A celestial power needs its bound skill (a plain swing is the default attack skill).
//! `Ally` targets the character the trigger names (the buffed one for `CastBuff`), else the
//! first friendly within `autoTargetRadius` (`SkillAutoCastController::FindAllyTarget`); with
//! no radius, nothing.
//!
//! On-hit passives: `Skill_PassiveOnHitBuffSelf::OnHitActivation` skips while its granted buff
//! runs; `Skill_PassiveOnHitBuffShield::OnHitActivation` also skips while its absorb pool has
//! anything left, and counts only a hit carrying one of its `…DamageQualifier` types
//! (`CombatAttributeAccumulator::GetDamageTypes` of the attack → `SkillProfile::IsQualifyingDamage`;
//! no qualifiers: any hit with damage). Both roll integer 0..=100 ≤ `onHitActivationChance`.
//!
//! Skill-level autocasts ([`SkillAutocasts`], monsters and pets): `Skill::LoadResources` creates
//! a child skill from `autoCastSkill` when `autoCastController` is set too; `Skill::Install`
//! sets it to the parent's level and registers a `SkillAutoCastController` with the parent's
//! id. `AttackingEnemy` then fires only for an attack made with the parent skill (or one it
//! grants, `IsGrantingSkill`) and never on a `Destructible`; `HitByEnemy` only while the
//! parent is active (`Skill.active`). `AppraiseAndExecute` rolls integer 0..=100 ≤
//! `chanceToRun` (no roll at 0); cooldown reduction applies (not an item skill).
//! PORT: `IsGrantingSkill` is not modelled (the attack's own skill must be the parent);
//! triggers other than AttackEnemy / HitBy* are not run for non-players.
use crate::combat::{AttackMade, HitLanded, MonsterKilled};
use bevy::prelude::*;
use gd_game_core::casting::{Casting, ProcCast};
use gd_game_core::combat::Combatant;
use gd_game_core::inventory::Inventory;
use gd_game_core::player::Player;
use std::sync::Arc;

pub struct AutocastPlugin;

impl Plugin for AutocastPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Triggered>().add_systems(
            Update,
            (
                collect_autocasts.run_if(resource_exists::<Inventory>),
                buff_autocasts,
                (trigger_autocasts, trigger_skill_autocasts).in_set(AutocastStage::Trigger),
            )
                .chain()
                .after(gd_game_core::combat::CombatSet),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Trigger {
    /// `Skill_PassiveOnLifeBuffSelf`: life at or below its threshold.
    LifeMonitor,
    AttackEnemy,
    AttackEnemyCrit,
    HitByEnemy,
    HitByMelee,
    HitByProjectile,
    HitByCrit,
    OnKill,
    Block,
    CastBuff,
    CastDebuf,
    LowHealth,
    LowMana,
    Other,
}

impl Trigger {
    /// A controller's trigger name (`triggerType`, `refreshCooldownTrigger`,
    /// `refreshDurationTrigger`).
    pub(crate) fn parse(name: &str) -> Trigger {
        match name {
            "AttackEnemy" => Trigger::AttackEnemy,
            "AttackEnemyCrit" => Trigger::AttackEnemyCrit,
            "HitByEnemy" => Trigger::HitByEnemy,
            "HitByMelee" => Trigger::HitByMelee,
            "HitByProjectile" => Trigger::HitByProjectile,
            "HitByCrit" => Trigger::HitByCrit,
            "OnKill" => Trigger::OnKill,
            "Block" => Trigger::Block,
            "CastBuff" => Trigger::CastBuff,
            "CastDebuf" => Trigger::CastDebuf,
            "LowHealth" => Trigger::LowHealth,
            "LowMana" => Trigger::LowMana,
            _ => Trigger::Other,
        }
    }

    /// A hit taken (`SkillAutoCastController::HitByEnemy`).
    pub(crate) fn taken(self) -> bool {
        matches!(
            self,
            Trigger::HitByEnemy
                | Trigger::HitByMelee
                | Trigger::HitByProjectile
                | Trigger::HitByCrit
                | Trigger::Block
        )
    }
}

/// One trigger event of the player this frame; the refresh controllers react to it as well.
#[derive(Message, Clone, Debug)]
pub(crate) struct Triggered {
    pub(crate) trigger: Trigger,
    /// The skill that attacked / killed (the default attack for a plain
    /// swing); None for hits taken.
    pub(crate) used: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Caster,
    Enemy,
    EnemyLocation,
    Ally,
}

#[derive(Clone, Debug)]
struct Auto {
    skill: String,
    level: u32,
    trigger: Trigger,
    chance: f32,
    param: f32,
    target: Target,
    radius: f32,
    /// Celestial power: only the bound skill triggers it.
    bound: Option<String>,
    /// Granted by a worn weapon (shields and off-hands included).
    weapon: bool,
    /// An item's skill (`Skill.isItemSkill`): its cooldown takes no cooldown reduction.
    item: bool,
    /// How `chance` is rolled: 0 controller `chanceToRun` ([`controller_roll`]), 1 integer 0..=100
    /// ≤ chance (`Skill_PassiveOnHitBuffSelf`), 2 uniform 0.1..100 ≤ chance
    /// (`Skill_OnHitAttackRadius`).
    roll: u8,
    /// A passive proc stays blocked while its buff runs (seconds).
    block: f32,
    /// `thresholdDuration` (life monitor): the buff's time only runs once
    /// life is back above the threshold.
    threshold: bool,
    /// `Skill_PassiveOnHitBuffShield`: the hit must carry a qualifying damage type
    /// (`SkillProfile::IsQualifyingDamage`); empty: any hit with damage.
    qualifiers: Vec<gd_sim::attributes::CombatType>,
    /// `Skill_PassiveOnHitBuffShield`: stays blocked while its absorb pool has anything left.
    shield: bool,
    /// Held by a running received buff (`buff_autocasts`), not by an item or skill.
    from_buff: bool,
}

#[derive(Component, Default)]
pub struct Autocasts {
    list: Vec<Auto>,
    inv_version: u64,
    dev_version: u64,
    skills_seen: usize,
    cooldowns: gd_game_core::casting::Cooldowns,
    /// LowHealth / LowMana: was below last frame (fires on crossing).
    low: [bool; 2],
    /// Passive procs whose buff is running: (skill, seconds left).
    active: Vec<(String, f32)>,
}

impl Autocasts {
    pub fn has(&self, skill: &str) -> bool {
        self.list
            .iter()
            .any(|a| a.skill.eq_ignore_ascii_case(skill))
    }

    pub fn cooldown(&self, skill: &str) -> Option<f32> {
        self.cooldowns.left_of(skill)
    }

    /// A passive proc's running buff got `secs` longer: it stays blocked as long.
    pub(crate) fn extend_block(&mut self, skill: &str, secs: f32) {
        self.active
            .iter_mut()
            .filter(|a| a.0.eq_ignore_ascii_case(skill))
            .for_each(|a| a.1 += secs);
    }

    /// A life monitor with `thresholdDuration` holds `skill`'s buff time while life
    /// (`life_pct` %) stays at or below its threshold.
    pub(crate) fn holds(&self, skill: &str, life_pct: f32) -> bool {
        self.list
            .iter()
            .any(|a| a.threshold && life_pct <= a.param && a.skill.eq_ignore_ascii_case(skill))
    }

    /// Disruption (`SkillManager::ApplyCooldownDamage`): disruptable procs cool down for
    /// `secs` unless they already do for longer.
    pub(crate) fn disrupt(&mut self, db: &gd_formats::vfs::Database, secs: f32) {
        let mut names: Vec<String> = self.list.iter().map(|a| a.skill.clone()).collect();
        names.sort();
        names.dedup();
        for n in names {
            if db.get(&n).is_some_and(gd_sim::skills::disruptable) {
                self.cooldowns.replace(&n, secs);
            }
        }
    }

    /// `SkillManager::RefreshCooldown`'s item-list walk.
    pub(crate) fn refresh_cooldowns(&mut self, secs: f32, except: &[String]) {
        self.cooldowns.refresh_all(secs, except);
    }
}

fn parse(
    db: &gd_formats::vfs::Database,
    skill: &str,
    controller: &str,
    level: u32,
    bound: Option<String>,
) -> Option<Auto> {
    let c = db.get(controller)?;
    let trigger = Trigger::parse(c.str("triggerType").unwrap_or(""));
    let target = match c.str("targetType").unwrap_or("Self") {
        "Enemy" => Target::Enemy,
        "EnemyLocation" => Target::EnemyLocation,
        "Ally" => Target::Ally,
        _ => Target::Caster,
    };
    Some(Auto {
        skill: skill.to_owned(),
        level: level.max(1),
        trigger,
        chance: c.f32("chanceToRun").unwrap_or(100.0),
        param: c.f32("triggerParam").unwrap_or(0.0),
        target,
        // Loaded without a default (`SkillAutoCastController` ctor): 0.
        radius: c.f32("autoTargetRadius").unwrap_or(0.0),
        bound,
        weapon: false,
        item: false,
        roll: 0,
        block: 0.0,
        threshold: false,
        from_buff: false,
        qualifiers: Vec::new(),
        shield: false,
    })
}

fn collect_autocasts(
    mut commands: Commands,
    inv: Res<Inventory>,
    // Option: devotion is optional (version 0 and no bindings without it).
    devotion: Option<Res<crate::devotion::DevotionState>>,
    data: Res<gd_game_core::data::GameData>,
    mut players: Query<
        (
            Entity,
            Option<&mut Autocasts>,
            &gd_game_core::player::Loadout,
        ),
        With<Player>,
    >,
) {
    let Ok((e, auto, loadout)) = players.single_mut() else {
        return;
    };
    let skills_seen = loadout
        .skills
        .skills
        .iter()
        .map(|s| s.1 as usize)
        .sum::<usize>()
        * 1000
        + loadout.skills.inactive.len()
        + loadout.skills.dispelled.len() * 100_000;
    let dev_version = devotion.as_ref().map_or(0, |d| d.version);
    if auto.as_ref().is_some_and(|a| {
        a.inv_version == inv.version && a.dev_version == dev_version && a.skills_seen == skills_seen
    }) {
        return;
    }
    let db = &data.db;
    let worn = inv.worn();
    let mut list = Vec::new();
    // Worn items' and sets' skills with a controller.
    let grants = gd_sim::skills::item_skill_grants(db, &worn);
    for g in &grants {
        let Some(ctl) = &g.controller else { continue };
        let level = gd_sim::skills::granted_level(&grants, &g.skill);
        // A `Weapon` item binds `AttackEnemy(Crit)` to attacks made with it.
        let weapon = g.item.is_some_and(|i| {
            db.get(&worn[i].base_name)
                .is_some_and(|b| b.class.starts_with("Weapon"))
        });
        list.extend(parse(db, &g.skill, ctl, level, None).map(|a| Auto {
            weapon,
            item: true,
            ..a
        }));
    }
    if let Some(d) = &devotion {
        for b in &d.bindings {
            list.extend(parse(
                db,
                &b.power,
                &b.controller,
                d.power_level(&b.power),
                Some(b.skill.clone()),
            ));
        }
    }
    // Passive procs of learned and item-granted skills (no controller).
    // `Skill_PassiveOnLifeBuffSelf::Update`: life at or below `lifeMonitorPercent` %, not
    // active; its buff time runs at once, or with `thresholdDuration` (default 1) only once
    // life is back above. `Skill_PassiveOnCritBuffSelf`: `SkillManager::OnCriticalAttack`,
    // integer 0..100 ≤ `onHitActivationChance`. Both buff for `skillActiveDuration`.
    let granted: Vec<&String> = grants
        .iter()
        .filter(|g| g.controller.is_none())
        .map(|g| &g.skill)
        .collect();
    let names: Vec<(String, u32)> = loadout
        .skills
        .skills
        .iter()
        .map(|(n, _)| (n.clone(), loadout.skills.level(n)))
        .chain(
            granted
                .iter()
                .map(|n| ((*n).clone(), gd_sim::skills::granted_level(&grants, n))),
        )
        .collect();
    for (name, level) in &names {
        let (name, level) = (name, *level);
        if level == 0 {
            continue;
        }
        let Some(r) = db.get(name) else { continue };
        let trigger = match r.class.as_str() {
            "Skill_PassiveOnLifeBuffSelf" => Trigger::LifeMonitor,
            "Skill_PassiveOnCritBuffSelf" => Trigger::AttackEnemyCrit,
            // `Skill_PassiveOnHitBuffSelf::OnHitActivation` / `Skill_PassiveOnHitBuffShield::OnHitActivation`.
            "Skill_PassiveOnHitBuffSelf" | "Skill_PassiveOnHitBuffShield" => Trigger::HitByEnemy,
            // Switched on: `Skill_OnHitAttackRadius::OnHitActivation`, uniform 0.1..100 ≤
            // `onHitActivationChance`, its own cooldown blocks it.
            "Skill_OnHitAttackRadius" if loadout.skill_active(&data.db, name) => {
                Trigger::HitByEnemy
            }
            _ => continue,
        };
        let li = level as usize - 1;
        let chance = if matches!(trigger, Trigger::HitByEnemy | Trigger::AttackEnemyCrit) {
            gd_game_core::casting::level_f(r, "onHitActivationChance", li)
        } else {
            100.0
        };
        let passive = r.class.starts_with("Skill_PassiveOn");
        list.push(Auto {
            skill: name.clone(),
            level,
            trigger,
            chance,
            param: gd_game_core::casting::level_f(r, "lifeMonitorPercent", li),
            target: Target::Caster,
            radius: 0.0,
            bound: None,
            weapon: false,
            item: false,
            roll: if r.class == "Skill_OnHitAttackRadius" {
                2
            } else {
                1
            },
            block: if passive {
                gd_game_core::casting::level_f(r, "skillActiveDuration", li)
            } else {
                0.0
            },
            threshold: trigger == Trigger::LifeMonitor
                && r.get("thresholdDuration")
                    .is_none_or(|_| r.bool("thresholdDuration")),
            from_buff: false,
            qualifiers: if r.class == "Skill_PassiveOnHitBuffShield" {
                gd_game_core::casting::qualifying_types(r)
            } else {
                Vec::new()
            },
            shield: r.class == "Skill_PassiveOnHitBuffShield",
        });
    }
    info!(
        "autocast: {} procs ({:?})",
        list.len(),
        list.iter()
            .map(|a| (a.skill.rsplit('/').next().unwrap_or(""), a.trigger))
            .collect::<Vec<_>>()
    );
    match auto {
        Some(mut a) => {
            a.list = list;
            a.inv_version = inv.version;
            a.dev_version = dev_version;
            a.skills_seen = skills_seen;
        }
        None => {
            commands.entity(e).try_insert(Autocasts {
                list,
                inv_version: inv.version,
                dev_version,
                skills_seen,
                ..default()
            });
        }
    }
}

/// A running received buff with an `autoCastSkill` / `autoCastController` (the Shattered
/// Realm's buff shrines, `SkillBuff_PassiveEndless`) gives the player that proc while it lasts,
/// at level 1 with no cooldown reduction (an item-style proc).
/// PORT: the engine's call that turns a buff's `autoCast*` fields into a controller is not read.
fn buff_autocasts(
    data: Res<gd_game_core::data::GameData>,
    mut players: Query<
        (
            &gd_game_core::skill_buffs::ReceivedBuffs,
            Option<&mut Autocasts>,
        ),
        With<Player>,
    >,
) {
    let Ok((buffs, Some(mut auto))) = players.single_mut() else {
        return;
    };
    let db = &data.db;
    let want: Vec<(String, String)> = buffs
        .list
        .iter()
        .filter_map(|b| {
            let r = db.get(&b.buff)?;
            Some((
                r.str("autoCastSkill").filter(|s| !s.is_empty())?.to_owned(),
                r.str("autoCastController")
                    .filter(|s| !s.is_empty())?
                    .to_owned(),
            ))
        })
        .collect();
    let have: Vec<(&str, ())> = auto
        .list
        .iter()
        .filter(|a| a.from_buff)
        .map(|a| (a.skill.as_str(), ()))
        .collect();
    if have.len() == want.len()
        && want
            .iter()
            .all(|w| have.iter().any(|h| h.0.eq_ignore_ascii_case(&w.0)))
    {
        return;
    }
    auto.list.retain(|a| !a.from_buff);
    for (skill, ctl) in &want {
        auto.list
            .extend(parse(db, skill, ctl, 1, None).map(|a| Auto {
                item: true,
                from_buff: true,
                ..a
            }));
    }
}

/// Which cast trigger a skill pulls when it applies its buff / debuff (the callers of
/// `SkillActivated::ApplyBuffOnTarget`): `Skill_BuffOther` / `Skill_ChargedBuffOther` buff,
/// `Skill_AttackBuff(Radius)` / `Skill_AttackProjectileDebuf` debuff, `Skill_BuffRadius(Toggled)`
/// by its `debufSkill` (`Skill_BuffRadius::Load`). Other skills, self buffs included, pull neither.
/// PORT: fires once per cast, not per buffed character.
fn buff_trigger(db: &gd_formats::vfs::Database, rec: &gd_formats::arz::Record) -> Option<Trigger> {
    match rec.class.as_str() {
        "Skill_BuffOther" | "Skill_ChargedBuffOther" => Some(Trigger::CastBuff),
        "Skill_AttackBuff" | "Skill_AttackBuffRadius" | "Skill_AttackProjectileDebuf" => {
            Some(Trigger::CastDebuf)
        }
        "Skill_BuffRadius" | "Skill_BuffRadiusToggled" => {
            Some(if crate::auras::is_debuf(db, rec) {
                Trigger::CastDebuf
            } else {
                Trigger::CastBuff
            })
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn trigger_autocasts(
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    mut landed: MessageReader<HitLanded>,
    mut made: MessageReader<AttackMade>,
    mut linked: MessageReader<crate::combat::LinkedAttack>,
    mut killed: MessageReader<MonsterKilled>,
    mut players: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            Option<&gd_game_core::casting::Caster>,
            &mut Autocasts,
            &gd_game_core::player::Loadout,
        ),
        With<Player>,
    >,
    casts: Query<
        (&Casting, Option<&gd_game_core::casting::CastWrapper>),
        (With<Player>, Added<Casting>),
    >,
    others: Query<
        (Entity, &Transform),
        (
            With<crate::combat::MonsterAi>,
            Without<gd_game_core::combat::Dead>,
        ),
    >,
    shields: Query<&gd_game_core::casting::Shields, With<Player>>,
    mut procs: MessageWriter<ProcCast>,
    mut triggered: MessageWriter<Triggered>,
) {
    let Ok((me, tf, cb, caster, mut auto, loadout)) = players.single_mut() else {
        landed.clear();
        made.clear();
        linked.clear();
        killed.clear();
        return;
    };
    let auto = &mut *auto;
    let dt = time.delta_secs();
    auto.cooldowns.tick(dt);
    let life_pct = 100.0 * cb.life / cb.profile.sheet.life.max(1.0);
    // A life monitor with `thresholdDuration` holds its time ([`Autocasts::holds`]).
    for i in 0..auto.active.len() {
        if !auto.holds(&auto.active[i].0, life_pct) {
            auto.active[i].1 -= dt;
        }
    }
    auto.active.retain(|a| a.1 > 0.0);
    // (trigger, skill used, enemy involved, weapon attack, hit damage types)
    let mut events: Vec<(
        Trigger,
        Option<Arc<str>>,
        Option<Entity>,
        bool,
        Option<&[gd_sim::attributes::CombatType]>,
    )> = Vec::new();
    for m in made.read() {
        if m.attacker == me {
            let used = m
                .skill
                .clone()
                .or_else(|| Some(Arc::from(gd_game_core::casting::DEFAULT_ATTACK)));
            events.push((Trigger::AttackEnemy, used.clone(), Some(m.target), m.weapon, None));
            if m.critical {
                events.push((Trigger::AttackEnemyCrit, used, Some(m.target), m.weapon, None));
            }
        }
    }
    for m in linked.read() {
        if m.attacker == me {
            let used = m
                .skill
                .clone()
                .or_else(|| Some(Arc::from(gd_game_core::casting::DEFAULT_ATTACK)));
            events.push((Trigger::AttackEnemy, used.clone(), Some(m.target), m.weapon, None));
            if m.critical {
                events.push((Trigger::AttackEnemyCrit, used, Some(m.target), m.weapon, None));
            }
        }
    }
    for h in landed.read() {
        if h.target == me {
            let types = Some(h.types.as_slice());
            events.push((Trigger::HitByEnemy, None, Some(h.attacker), false, types));
            events.push((
                if h.melee {
                    Trigger::HitByMelee
                } else {
                    Trigger::HitByProjectile
                },
                None,
                Some(h.attacker),
                false,
                types,
            ));
            if h.critical {
                events.push((Trigger::HitByCrit, None, Some(h.attacker), false, types));
            }
            if h.blocked {
                events.push((Trigger::Block, None, Some(h.attacker), false, types));
            }
        }
    }
    // OnKill: `used` is the killing skill (bound powers need their skill).
    for k in killed.read() {
        if let Some((e, skill)) = &k.killer
            && *e == me
        {
            events.push((
                Trigger::OnKill,
                Some(
                    skill
                        .as_ref()
                        .map(|s| Arc::from(s.as_str()))
                        .unwrap_or_else(|| Arc::from(gd_game_core::casting::DEFAULT_ATTACK)),
                ),
                None,
                false,
                None,
            ));
        }
    }
    for (c, w) in &casts {
        // A hot slot cast runs as the wrapper's buff record: the wrapper's class decides.
        let name = w
            .filter(|w| w.buff == c.skill)
            .map_or(&c.skill, |w| &w.wrapper);
        let Some(t) = data.db.get(name).and_then(|r| buff_trigger(&data.db, r)) else {
            continue;
        };
        events.push((t, Some(Arc::from(name.as_str())), None, false, None));
    }
    if cb.life > 0.0
        && auto
            .list
            .iter()
            .any(|a| a.trigger == Trigger::LifeMonitor && life_pct <= a.param)
    {
        events.push((Trigger::LifeMonitor, None, None, false, None));
    }
    let energy_pct = caster.map_or(100.0, |c| {
        100.0 * c.energy / cb.profile.sheet.energy.max(1.0)
    });
    for (k, (t, pct)) in [
        (Trigger::LowHealth, life_pct),
        (Trigger::LowMana, energy_pct),
    ]
    .into_iter()
    .enumerate()
    {
        let below = auto.list.iter().any(|a| a.trigger == t && pct < a.param);
        if below && !auto.low[k] {
            events.push((t, None, None, false, None));
        }
        auto.low[k] = below;
    }
    triggered.write_batch(events.iter().filter(|e| e.0 != Trigger::LifeMonitor).map(
        |(t, used, ..)| Triggered {
            trigger: *t,
            used: used.clone(),
        },
    ));
    if events.is_empty() {
        return;
    }
    for a in auto.list.clone() {
        for (t, used, enemy, weapon, hit_types) in &events {
            if a.trigger != *t {
                continue;
            }
            if a.trigger == Trigger::LifeMonitor && life_pct > a.param {
                continue;
            }
            if a.weapon && matches!(t, Trigger::AttackEnemy | Trigger::AttackEnemyCrit) && !weapon {
                continue;
            }
            // Bound powers: only their skill (`CastingBuff` / `CastingDebuf` do not check);
            // hits taken count while the bound skill is active (`Skill.active`).
            let taken = t.taken();
            let active = |b: &str| loadout.skill_active(&data.db, b);
            if let Some(b) = &a.bound
                && !matches!(t, Trigger::CastBuff | Trigger::CastDebuf)
                && !(taken && active(b))
                && !used.as_ref().is_some_and(|u| u.eq_ignore_ascii_case(b))
            {
                continue;
            }
            if a.shield {
                // The shield's own absorb pool blocks it while anything is left
                // (`Skill_PassiveOnHitBuffShield::OnHitActivation`).
                if shields.get(me).is_ok_and(|s| {
                    s.0.iter().any(|x| {
                        x.skill.eq_ignore_ascii_case(&a.skill) && !x.absorb.spent()
                    })
                }) {
                    continue;
                }
                // Only a hit carrying a qualifying damage type counts
                // (`SkillProfile::IsQualifyingDamage`; no qualifiers: any damage).
                let Some(types) = hit_types else { continue };
                if types.is_empty()
                    || (!a.qualifiers.is_empty()
                        && !types.iter().any(|ty| a.qualifiers.contains(ty)))
                {
                    continue;
                }
            }
            if !auto.cooldowns.usable(&a.skill) || auto.active.iter().any(|c| c.0 == a.skill) {
                continue;
            }
            let passes = match a.roll {
                1 => gd_game_core::rng::u32(0..=100) as f32 <= a.chance,
                2 => 0.1 + gd_game_core::rng::f32() * 99.9 <= a.chance,
                _ => controller_roll(a.chance),
            };
            if !passes {
                continue;
            }
            let Some(rec) = data.db.get(&a.skill) else {
                continue;
            };
            let li = (a.level - 1) as usize;
            // The enemy involved, else the nearest in range.
            let foe = enemy.and_then(|e| others.get(e).ok()).or_else(|| {
                others
                    .iter()
                    .filter(|(_, t)| t.translation.distance(tf.translation) <= a.radius)
                    .min_by(|x, y| {
                        x.1.translation
                            .distance(tf.translation)
                            .total_cmp(&y.1.translation.distance(tf.translation))
                    })
            });
            let (point, target) = match a.target {
                Target::Caster => (tf.translation, None),
                // The buffed character for `CastBuff`, else the first friend within the
                // radius: the player stands inside its own.
                Target::Ally if matches!(t, Trigger::CastBuff) || a.radius > 0.0 => {
                    (tf.translation, None)
                }
                Target::Ally => continue,
                Target::Enemy => match foe {
                    Some((e, t)) => (t.translation, Some(e)),
                    None => continue,
                },
                Target::EnemyLocation => match foe {
                    Some((_, t)) => (t.translation, None),
                    None => continue,
                },
            };
            let (cd, charges) = proc_cooldown(&data.db, rec, &a.skill, li, a.item, cb);
            auto.cooldowns.start(a.skill.clone(), cd, charges);
            if a.block > 0.0 {
                auto.active.push((a.skill.clone(), a.block));
            }
            debug!("autocast {} ({:?})", a.skill, t);
            procs.write(ProcCast {
                caster: me,
                cast: Casting {
                    skill: a.skill.clone(),
                    level_index: li,
                    kind: proc_kind(rec),
                    point,
                    target,
                    hostile: false,
                    parent: None,
                },
            });
            break;
        }
    }
}

/// A proc's cooldown (`Skill::GetCooldownTime`): the profile record's cooldown (a wrapper's
/// `buffSkillName`) × (1 − `skillCooldownReduction` %), the skill's own plus the character's;
/// item skills (`Skill.isItemSkill`) take no reduction.
fn proc_cooldown(
    db: &gd_formats::vfs::Database,
    rec: &gd_formats::arz::Record,
    skill: &str,
    li: usize,
    item: bool,
    cb: &Combatant,
) -> (f32, u32) {
    let cdr = if item {
        0.0
    } else {
        gd_game_core::casting::own_skill_attribute(
            std::iter::once((rec, li)),
            "skillCooldownReduction",
        ) + cb.profile.skill_attribute("skillCooldownReduction")
    };
    let profile = gd_sim::skills::profile_record(db, skill).unwrap_or_else(|| rec.clone());
    let cd = gd_game_core::casting::level_f(&profile, "skillCooldownTime", li) * (1.0 - cdr * 0.01);
    let charges = gd_game_core::casting::level_f(&profile, "cooldownCharges", li).max(1.0) as u32;
    (cd, charges)
}

/// `SkillAutoCastController::AppraiseAndExecute`'s roll: no roll at a chance ≤ 0 (it fires),
/// else integer 0..=100 ≤ chance.
fn controller_roll(chance: f32) -> bool {
    chance <= 0.0 || gd_game_core::rng::u32(0..=100) as f32 <= chance
}

/// Passive self-buff procs fire as buffs, `Skill_OnHitAttackRadius` as a radius attack, the
/// rest by class.
fn proc_kind(rec: &gd_formats::arz::Record) -> gd_game_core::casting::Kind {
    match rec.class.as_str() {
        c if c.starts_with("Skill_PassiveOn") => gd_game_core::casting::Kind::Buff,
        "Skill_OnHitAttackRadius" => gd_game_core::casting::Kind::SelfRadius,
        _ => gd_game_core::casting::kind_of(rec),
    }
}

/// A monster's or pet's skill-level autocasts (module docs) and their cooldowns.
#[derive(Component, Default, Debug)]
pub struct SkillAutocasts {
    list: Vec<Auto>,
    cooldowns: gd_game_core::casting::Cooldowns,
}

impl SkillAutocasts {
    /// The autocast children of a character's skills `(record, level)`; None when it has none.
    pub(crate) fn of(db: &gd_formats::vfs::Database, skills: &[(String, u32)]) -> Option<Self> {
        let list: Vec<Auto> = skills
            .iter()
            .filter(|(_, level)| *level > 0)
            .filter_map(|(parent, level)| {
                let r = db.get(parent)?;
                let child = r.str("autoCastSkill").filter(|s| !s.is_empty())?;
                let ctl = r.str("autoCastController").filter(|s| !s.is_empty())?;
                parse(db, child, ctl, *level, Some(parent.clone()))
            })
            .collect();
        (!list.is_empty()).then(|| Self {
            list,
            cooldowns: gd_game_core::casting::Cooldowns::default(),
        })
    }

    pub fn has(&self, skill: &str) -> bool {
        self.list
            .iter()
            .any(|a| a.skill.eq_ignore_ascii_case(skill))
    }

    /// `SkillManager::RefreshCooldown`'s sub-skill-list walk.
    pub(crate) fn refresh_cooldowns(&mut self, secs: f32, except: &[String]) {
        self.cooldowns.refresh_all(secs, except);
    }
}

/// Runs the [`SkillAutocasts`] of monsters and pets on their attacks and the hits they take.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn trigger_skill_autocasts(
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    mut landed: MessageReader<HitLanded>,
    mut made: MessageReader<AttackMade>,
    mut linked: MessageReader<crate::combat::LinkedAttack>,
    mut owners: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            &mut SkillAutocasts,
            Option<&crate::monster_skills::MonsterSkills>,
            Option<&crate::pets::Pet>,
            Option<&gd_game_core::casting::DamageAuras>,
        ),
        (Without<Player>, Without<gd_game_core::combat::Dead>),
    >,
    all: Query<
        (Entity, &Transform, crate::combat::VictimSide),
        Without<gd_game_core::combat::Dead>,
    >,
    sides: Query<crate::combat::SideData>,
    mut procs: MessageWriter<ProcCast>,
) {
    let dt = time.delta_secs();
    // (owner, trigger, skill used, character involved)
    let mut events: Vec<(Entity, Trigger, Option<Arc<str>>, Entity)> = Vec::new();
    let mut attack =
        |attacker: Entity, target: Entity, skill: &Option<Arc<str>>, critical: bool| {
            if !owners.contains(attacker) {
                return;
            }
            let used = skill
                .clone()
                .or_else(|| Some(Arc::from(gd_game_core::casting::DEFAULT_ATTACK)));
            if critical {
                events.push((attacker, Trigger::AttackEnemyCrit, used.clone(), target));
            }
            events.push((attacker, Trigger::AttackEnemy, used, target));
        };
    for m in made.read() {
        attack(m.attacker, m.target, &m.skill, m.critical);
    }
    for m in linked.read() {
        attack(m.attacker, m.target, &m.skill, m.critical);
    }
    for h in landed.read() {
        if !owners.contains(h.target) {
            continue;
        }
        let style = if h.melee {
            Trigger::HitByMelee
        } else {
            Trigger::HitByProjectile
        };
        for (t, on) in [
            (Trigger::HitByEnemy, true),
            (style, true),
            (Trigger::HitByCrit, h.critical),
            (Trigger::Block, h.blocked),
        ] {
            if on {
                events.push((h.target, t, None, h.attacker));
            }
        }
    }
    for (me, tf, cb, mut auto, ms, pet, auras) in &mut owners {
        let auto = &mut *auto;
        auto.cooldowns.tick(dt);
        let side = crate::combat::Side::lookup(&sides, me, pet.is_none());
        // The parent skill's `Skill.active`: a toggle on, a timed buff or damage aura running.
        let active = |s: &str| {
            ms.is_some_and(|m| m.skill_running(s))
                || pet.is_some_and(|p| p.skill_running(s))
                || auras.is_some_and(|a| a.0.iter().any(|x| x.skill().eq_ignore_ascii_case(s)))
        };
        for a in auto.list.clone() {
            for (_, t, used, other) in events.iter().filter(|e| e.0 == me) {
                if a.trigger != *t {
                    continue;
                }
                // Bound to the parent (controller type 2).
                let parent = a.bound.as_deref().unwrap_or("");
                let bound_ok = if t.taken() {
                    active(parent)
                } else {
                    used.as_ref()
                        .is_some_and(|u| u.eq_ignore_ascii_case(parent))
                };
                if !bound_ok || !auto.cooldowns.usable(&a.skill) {
                    continue;
                }
                if !controller_roll(a.chance) {
                    continue;
                }
                let Some(rec) = data.db.get(&a.skill) else {
                    continue;
                };
                let li = (a.level - 1) as usize;
                // The character involved (`AttackingEnemy` skips a `Destructible`), else the
                // nearest foe within `autoTargetRadius`.
                let foe = all
                    .get(*other)
                    .ok()
                    .filter(|(_, _, v)| v.character() && side.hits(v))
                    .map(|(e, t, _)| (e, t.translation))
                    .or_else(|| {
                        all.iter()
                            .filter(|(e, t, v)| {
                                *e != me
                                    && v.character()
                                    && side.hits(v)
                                    && t.translation.distance(tf.translation) <= a.radius
                            })
                            .map(|(e, t, _)| (e, t.translation))
                            .min_by(|x, y| {
                                x.1.distance(tf.translation)
                                    .total_cmp(&y.1.distance(tf.translation))
                            })
                    });
                let (point, target) = match a.target {
                    Target::Caster => (tf.translation, None),
                    Target::Ally if a.radius > 0.0 => (tf.translation, None),
                    Target::Ally => continue,
                    Target::Enemy => match foe {
                        Some((e, at)) => (at, Some(e)),
                        None => continue,
                    },
                    Target::EnemyLocation => match foe {
                        Some((_, at)) => (at, None),
                        None => continue,
                    },
                };
                let (cd, charges) = proc_cooldown(&data.db, rec, &a.skill, li, false, cb);
                auto.cooldowns.start(a.skill.clone(), cd, charges);
                debug!("{me}: skill autocast {} ({t:?}, {parent})", a.skill);
                procs.write(ProcCast {
                    caster: me,
                    cast: Casting {
                        skill: a.skill.clone(),
                        level_index: li,
                        kind: proc_kind(rec),
                        point,
                        target,
                        hostile: side.hostile(),
                        parent: None,
                    },
                });
                break;
            }
        }
    }
}

/// Autocasts triggered: what reacts orders after `Trigger`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutocastStage {
    Trigger,
}
