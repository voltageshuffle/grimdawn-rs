//! Cooldown and duration refreshes: a skill with `refreshCooldownSkill` / `refreshDurationSkill` cuts another
//! skill's cooldown or lengthens its running buff on a trigger.
//!
//! Fields (`Skill::LoadResources`, `SkillProfile_Modifiers::Load`): `refreshCooldownSkill` / `…Trigger` /
//! `…Chance` (integer %) / `…Amount` (s), and `refreshDurationSkill` / `…Trigger` / `…Chance` / `…Amount` (s) /
//! `…Max` (s). Triggers in stock: `AttackEnemy`, `AttackEnemyCrit`, `HitByEnemy`, `Block`, `OnKill`.
//!
//! `Skill::Install`: when the player knows the named skill (`SkillManager::FindSkillId`), it registers a
//! `RefreshSkillCooldownController` / `RefreshSkillDurationController` with the chance and amount of the installing
//! skill's level. Both are `SkillAutoCastController`s with the procs' triggers ([`crate::autocast::Triggered`]) and
//! `AppraiseAndExecute`: a chance ≤ 0 always passes, else `RandomUniform(0, 100) ≤ chance`; no cooldown of their own.
//!
//! Whose events count: the controller's skill list is the installing modifier's parents (`Skill.parentSkills`),
//! else the skill itself. `AttackingEnemy` / `EnemyDeath` take the attacking / killing skill when it is one of them,
//! or granted by one (`IsGrantingSkill`), or while one of them is active
//! ([`gd_game_core::player::Loadout::skill_active`]): an aura's modifier fires on any attack while the aura runs.
//! `HitByEnemy` (hits taken, blocks) needs one of them active. A refresh may fire from attacks of the skill it
//! refreshes.
//!
//! The effect (`RefreshSkillController::Execute`): a cooldown refresh is ready only while the skill cools down and
//! runs `Skill::EndCooldown(amount × 1000 ms)` (the time left only). A duration refresh is ready only while the
//! skill's buff runs and calls `SkillBuff::AddTimeToLive`, which does nothing unless the whole amount fits: time
//! left + amount ≤ the buff's full duration. `refreshDurationMax` is a budget per buff instance
//! (`Skill::GetRefreshDurationModifierData`, filled when the buff starts): the skill's own max under its own id,
//! each registered modifier's under its parent, or summed under key 0 when the parent is a self buff ([`POOLED`]).
//! An update from source `s` is cut to what is left of `s`'s entry and spends it, then to what is left of key 0's
//! and spends that; a key with no entry cuts nothing. The source is the attacking skill for `AttackEnemy(Crit)`
//! only; other triggers pass none (key 0), so their extension is cut by the pool, which it spends twice.
//!
//! PORT: `LowHealth` / `LowMana` refreshes are not raised (no stock record uses them); the hits taken by a
//! `Monster` the controller may ignore are not told apart; the budget's buff instance is told by its time left
//! jumping up (a recast).
use crate::autocast::{Trigger, Triggered};
use bevy::prelude::*;
use gd_game_core::casting::Caster;
use gd_game_core::casting::level_f;
use gd_game_core::player::{EffectiveSkills, Loadout, Player};
use gd_game_core::weapon_pool::SkillLinks;
use gd_sim::skills::SkillBook;

pub struct RefreshPlugin;

impl Plugin for RefreshPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (collect_refreshers, apply_refreshes)
                .chain()
                .after(crate::autocast::AutocastStage::Trigger),
        );
    }
}

#[derive(Clone, Debug)]
struct Refresher {
    /// Duration (else cooldown).
    duration: bool,
    /// The refreshed skill under its names (the record, its `buffSkillName` profile).
    target: Vec<String>,
    trigger: Trigger,
    chance: f32,
    amount: f32,
    /// `refreshDurationMax` (s), 0 for none.
    max: f32,
    /// The controller's skill list (lower-case): the parents, else itself.
    skills: Vec<String>,
}

/// A running buff's `refreshDurationMax` budget: what is left per key (a skill's lower-case name; `""` the pool
/// shared by the modifiers of self buffs).
#[derive(Clone, Debug)]
struct Budget {
    buff: String,
    /// Its time left after the last extension (a recast makes it jump up).
    seen: f32,
    left: Vec<(String, f32)>,
}

/// `Skill::GetRefreshDurationModifierData`: the modifiers of a skill of these classes (or one derived from them)
/// pool their `refreshDurationMax` under key 0 instead of their parent's id.
const POOLED: &[&str] = &[
    "Skill_BuffAttackRadiusDuration",
    "Skill_BuffAttackRadiusLightning",
    "Skill_BuffAttackRadiusToggled",
    "Skill_BuffRadius",
    "Skill_BuffRadiusToggled",
    "Skill_BuffSelfColossus",
    "Skill_BuffSelfDuration",
    "Skill_BuffSelfImmobilize",
    "Skill_BuffSelfInvulnerable",
    "Skill_BuffSelfShield",
    "Skill_BuffSelfSick",
    "Skill_BuffSelfToggled",
    "Skill_ChargePotion",
    "Skill_OnHitAttackRadius",
    "Skill_OnHitBuffSelf",
    "Skill_PassiveOnCritBuffSelf",
    "Skill_PassiveOnHitBuffSelf",
    "Skill_PassiveOnHitBuffShield",
    "Skill_RefreshCooldown",
    "Skill_Shapeshift",
    "Skill_Teleport",
    "SkillActivatedBuffSelf",
];

/// The budget entries of a buff instance: the target's own `refreshDurationMax` under its own name, then each
/// registered modifier's under its parent, or summed under `""` when the parent is a self buff ([`POOLED`]).
fn budget_entries(
    db: &gd_formats::vfs::Database,
    list: &[Refresher],
    r: &Refresher,
    own: f32,
) -> Vec<(String, f32)> {
    let mut left: Vec<(String, f32)> = Vec::new();
    let mut add = |key: String, secs: f32| match left.iter_mut().find(|e| e.0 == key) {
        Some(e) => e.1 += secs,
        None => left.push((key, secs)),
    };
    if own > 0.0 {
        add(r.target[0].clone(), own);
    }
    for o in list
        .iter()
        .filter(|o| o.duration && o.target == r.target && o.max > 0.0)
    {
        let Some(parent) = o.skills.first() else {
            continue;
        };
        let pooled = db
            .get(parent)
            .is_some_and(|p| POOLED.contains(&p.class.as_str()));
        add(
            if pooled {
                String::new()
            } else {
                parent.clone()
            },
            o.max,
        );
    }
    left
}

/// The player's refresh controllers and the budgets of the buffs they lengthened.
#[derive(Component, Default)]
pub struct Refreshers {
    list: Vec<Refresher>,
    budgets: Vec<Budget>,
}

fn names(db: &gd_formats::vfs::Database, skill: &str) -> Vec<String> {
    let mut v = vec![skill.to_ascii_lowercase()];
    if let Some(p) = db
        .get(skill)
        .and_then(|r| gd_sim::skills::profile_name(r).map(str::to_ascii_lowercase))
    {
        v.push(p);
    }
    v
}

/// The controllers a skill at `li` registers (`Skill::Install`).
fn controllers(
    db: &gd_formats::vfs::Database,
    loadout: &Loadout,
    effective: &SkillBook,
    skill: &str,
    li: usize,
    skills: Vec<String>,
) -> Vec<Refresher> {
    let Some(r) = db.get(skill) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (duration, p) in [(false, "refreshCooldown"), (true, "refreshDuration")] {
        let Some(target) = r.str(&format!("{p}Skill")).filter(|s| !s.is_empty()) else {
            continue;
        };
        // `FindSkillId`: the character must have the skill.
        if loadout.skill_level(db, effective, target) == 0 {
            continue;
        }
        out.push(Refresher {
            duration,
            target: names(db, target),
            trigger: Trigger::parse(r.str(&format!("{p}Trigger")).unwrap_or("")),
            chance: level_f(r, &format!("{p}Chance"), li).trunc(),
            amount: level_f(r, &format!("{p}Amount"), li),
            max: if duration {
                level_f(r, "refreshDurationMax", li)
            } else {
                0.0
            },
            skills: skills.clone(),
        });
    }
    out
}

#[allow(clippy::type_complexity)]
fn collect_refreshers(
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    // Option: without an inventory only the worn items' refreshers are skipped.
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
    mut players: Query<
        (
            Entity,
            &SkillLinks,
            &Loadout,
            Option<&EffectiveSkills>,
            Option<&mut Refreshers>,
        ),
        (With<Player>, Changed<SkillLinks>),
    >,
) {
    let Ok((e, links, loadout, effective, refreshers)) = players.single_mut() else {
        return;
    };
    let db = &data.db;
    let rebuilt;
    let effective = match effective {
        Some(e) => &e.0,
        None => {
            rebuilt = loadout.effective_skills(db, &data.skill_trees());
            &rebuilt
        }
    };
    // Modifiers (learned and gear) with the skills they modify.
    let mut mods: Vec<(String, usize, Vec<String>)> = Vec::new();
    for (parent, list) in &links.mods {
        for (m, li) in list {
            match mods.iter_mut().find(|x| x.0.eq_ignore_ascii_case(m)) {
                Some(x) => {
                    if !x.2.contains(parent) {
                        x.2.push(parent.clone());
                    }
                }
                None => mods.push((m.clone(), *li, vec![parent.clone()])),
            }
        }
    }
    let mut list: Vec<Refresher> = mods
        .into_iter()
        .flat_map(|(m, li, parents)| controllers(db, loadout, effective, &m, li, parents))
        .collect();
    // Skills granted by worn items: their own list is themselves.
    if let Some(inv) = &inv {
        let worn = inv.worn();
        let grants = gd_sim::skills::item_skill_grants(db, &worn);
        for g in &grants {
            let level = gd_sim::skills::granted_level(&grants, &g.skill).max(1);
            list.extend(controllers(
                db,
                loadout,
                effective,
                &g.skill,
                level as usize - 1,
                names(db, &g.skill),
            ));
        }
    }
    debug!(
        "refresh: {} controllers ({:?})",
        list.len(),
        list.iter()
            .map(|r| (
                r.target[0].rsplit('/').next().unwrap_or(""),
                r.trigger,
                r.duration
            ))
            .collect::<Vec<_>>()
    );
    match refreshers {
        Some(mut r) => r.list = list,
        None => {
            commands.entity(e).try_insert(Refreshers {
                list,
                budgets: Vec::new(),
            });
        }
    }
}

fn apply_refreshes(
    data: Res<gd_game_core::data::GameData>,
    mut events: MessageReader<Triggered>,
    mut players: Query<
        (
            &mut Refreshers,
            &mut Caster,
            &mut Loadout,
            &SkillLinks,
            Option<&EffectiveSkills>,
            Option<&mut crate::autocast::Autocasts>,
        ),
        With<Player>,
    >,
) {
    let Ok((mut refreshers, mut caster, mut loadout, links, effective, mut autocasts)) =
        players.single_mut()
    else {
        events.clear();
        return;
    };
    let refreshers = &mut *refreshers;
    // A budget lives as long as its buff instance.
    refreshers.budgets.retain(|b| {
        loadout
            .buffs
            .list
            .iter()
            .any(|x| x.0.eq_ignore_ascii_case(&b.buff) && x.2 <= b.seen + 1e-3)
    });
    for ev in events.read() {
        for r in refreshers.list.iter().filter(|r| r.trigger == ev.trigger) {
            let active = || r.skills.iter().any(|s| loadout.skill_active(&data.db, s));
            let counts = if ev.trigger.taken() {
                active()
            } else if matches!(
                ev.trigger,
                Trigger::AttackEnemy | Trigger::AttackEnemyCrit | Trigger::OnKill
            ) {
                let used = ev
                    .used
                    .as_deref()
                    .unwrap_or(gd_game_core::casting::DEFAULT_ATTACK)
                    .to_ascii_lowercase();
                r.skills.contains(&used)
                    || r.skills.iter().any(|s| {
                        links
                            .all_secondaries_of(s)
                            .any(|(sec, _)| sec.eq_ignore_ascii_case(&used))
                    })
                    || active()
            } else {
                true
            };
            if !counts {
                continue;
            }
            let buff = r
                .duration
                .then(|| {
                    loadout
                        .buffs
                        .list
                        .iter()
                        .position(|b| r.target.iter().any(|t| t.eq_ignore_ascii_case(&b.0)))
                })
                .flatten();
            let ready = if r.duration {
                buff.is_some()
            } else {
                caster.cooldowns.any_of(&r.target)
            };
            if !ready {
                continue;
            }
            if r.chance > 0.0 && gd_game_core::rng::u32(0..=100) as f32 > r.chance {
                continue;
            }
            if !r.duration {
                if r.amount > 0.0 {
                    caster.cooldowns.shorten(&r.target, r.amount);
                    debug!(
                        "refresh: {} cooldown -{} s",
                        r.target[0].rsplit('/').next().unwrap_or(""),
                        r.amount
                    );
                }
                continue;
            }
            let i = buff.expect("ready");
            let name = loadout.buffs.list[i].0.clone();
            let amount = r.amount.max(0.0);
            // Only when the whole amount fits under the buff's full duration.
            let time_left = loadout.buffs.list[i].2;
            let full = loadout.buffs.full(&name).unwrap_or(time_left);
            if amount <= 0.0 || time_left + amount > full + 1e-4 {
                continue;
            }
            // The source: the attacking skill for attacks, else none (key 0, the pool).
            let src = ev
                .used
                .as_deref()
                .filter(|_| matches!(ev.trigger, Trigger::AttackEnemy | Trigger::AttackEnemyCrit))
                .map(str::to_ascii_lowercase)
                .map(|s| {
                    if r.target.contains(&s) {
                        r.target[0].clone()
                    } else {
                        s
                    }
                })
                .unwrap_or_default();
            let b = match refreshers
                .budgets
                .iter()
                .position(|b| b.buff.eq_ignore_ascii_case(&name))
            {
                Some(j) => j,
                None => {
                    let own = data.db.get(&r.target[0]).map_or(0.0, |t| {
                        let level =
                            effective.map_or(1, |e| loadout.skill_level(&data.db, e, &r.target[0]));
                        level_f(t, "refreshDurationMax", level.max(1) as usize - 1)
                    });
                    refreshers.budgets.push(Budget {
                        buff: name.clone(),
                        seen: time_left,
                        left: budget_entries(&data.db, &refreshers.list, r, own),
                    });
                    refreshers.budgets.len() - 1
                }
            };
            // The source's entry, then the pool's (for no source the same entry, spent twice).
            let mut add = amount;
            for key in [src.as_str(), ""] {
                if add <= 0.0 {
                    break;
                }
                if let Some(e) = refreshers.budgets[b].left.iter_mut().find(|e| e.0 == key) {
                    add = add.min(e.1).max(0.0);
                    e.1 -= add;
                }
            }
            if add <= 0.0 {
                continue;
            }
            loadout.buffs.list[i].2 = (time_left + add).min(full);
            refreshers.budgets[b].seen = loadout.buffs.list[i].2;
            if let Some(a) = autocasts.as_mut() {
                a.extend_block(&name, add);
            }
            debug!(
                "refresh: {} +{add} s",
                name.rsplit('/').next().unwrap_or("")
            );
        }
    }
}
