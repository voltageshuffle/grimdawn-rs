//! Cooldown and duration refreshes: a skill (in stock: `Skill_Modifier`s of
//! the class trees and of gear, one `Skill_Transmuter`, two item
//! `Skill_WPAttack_BasicAttack`s) with `refreshCooldownSkill` /
//! `refreshDurationSkill` cuts another skill's cooldown or lengthens its
//! running buff on a trigger.
//!
//! Fields (`Skill::LoadResources` @0x18048e810 reads the names and triggers,
//! gdx3 only; the profile `sub_18054a660` the per-level values):
//! `refreshCooldownSkill` / `refreshCooldownTrigger` /
//! `refreshCooldownChance` (integer %) / `refreshCooldownAmount` (s), and
//! `refreshDurationSkill` / `refreshDurationTrigger` /
//! `refreshDurationChance` / `refreshDurationAmount` (s) /
//! `refreshDurationMax` (s). Triggers in stock: `AttackEnemy`,
//! `AttackEnemyCrit`, `HitByEnemy`, `Block`, `OnKill`.
//!
//! `Skill::Install` @0x1804813f0: when the character's skill
//! manager knows the named skill (`SkillManager::FindSkillId`, the player
//! only: `RegisterRefreshSkillCooldown` @0x18053e040 / `…Duration`), it
//! registers a `RefreshSkillCooldownController` @0x1805244d0 /
//! `RefreshSkillDurationController` @0x180523d90 with the chance and amount
//! of the installing skill's level. Both are `SkillAutoCastController`s:
//! the same trigger objects as the procs (`crate::autocast`, whose trigger
//! events [`crate::autocast::Triggered`] this module reads), the same
//! `AppraiseAndExecute` @0x180523440: a chance ≤ 0 always passes, else
//! `RandomUniform(0, 100) ≤ chance`; no cooldown of their own (the action
//! never sets its block flag for a non-zero amount).
//!
//! Whose events count (controller type 2, a skill's): the controller's
//! skill list is the installing modifier's parents (`Skill` +440, the base
//! skill before it in the tree or the gear's `modifiedSkillName`,
//! `SkillManager::CreateAndLoadSkill` @0x18052f300), else the skill itself.
//! `AttackingEnemy` @0x180522810 / `EnemyDeath` @0x180522ae0 take the
//! attacking / killing skill when it is one of them, or granted by one
//! (`IsGrantingSkill` @0x180522c80: its buff, pool attack, secondary or
//! sub-skill), or while one of them is active (`Skill` +0xDC:
//! [`crate::player::Loadout::skill_active`]): an aura's modifier fires on
//! any attack while the aura runs. `HitByEnemy` @0x180522580 (hits taken,
//! blocks) needs one of them active. A refresh may fire from attacks of
//! the skill it refreshes (`SkillServices_Character::AttackingEnemy`
//! @0x180559ab0 skips that only for procs).
//!
//! The effect (`sub_1805257b0`): a cooldown refresh is ready only while the
//! skill cools down and runs `Skill::EndCooldown(amount × 1000 ms)`
//! ([`crate::casting::Cooldowns::shorten`]: the time left only). A
//! duration refresh is ready only while the skill's buff runs
//! (`SkillManager::IsSkillBuffActive`) and sends the buff an update with
//! the amount (`sub_1805258f0` → `SkillServices_Character::
//! CreateUpdateSkillBuff` @0x1805586f0 → `SkillBuff::AddTimeToLive`
//! @0x180526f90): the time left grows by the amount, not capped at the
//! buff's full duration. `refreshDurationMax` is a budget per buff
//! instance (`Skill::GetRefreshDurationModifierData` @0x1804903d0, filled
//! when the buff starts): per key, the skill's own max under its own id and
//! each registered modifier's under its parent; an update from source `s`
//! adds at most what is left of `s`'s budget and spends it, a source with no
//! budget adds the full amount. The source is the attacking skill for
//! `AttackEnemy(Crit)` only (the action's +32, set by `AttackingEnemy`);
//! other triggers pass none, so their extensions are never capped.
//!
//! PORT: `LowHealth` / `LowMana` refreshes are not raised (no stock record
//! uses them; `crate::autocast` raises those only for its procs);
//! the hits taken by a `Monster` the controller may ignore (`HitByEnemy`'s
//! vtable +1128 / +2256 test) are not told apart; the budget's buff
//! instance is told by its time left jumping up (a recast).
use crate::autocast::{Trigger, Triggered};
use crate::casting::{Caster, level_f};
use crate::player::{EffectiveSkills, Loadout, Player};
use crate::weapon_pool::SkillLinks;
use bevy::prelude::*;
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

/// One registered refresh controller.
#[derive(Clone, Debug)]
struct Refresher {
    /// Duration (else cooldown).
    duration: bool,
    /// The refreshed skill under its names (the record, its `buffSkillName`
    /// profile: hot slot casts run, cool down and buff as either).
    target: Vec<String>,
    trigger: Trigger,
    chance: f32,
    amount: f32,
    /// `refreshDurationMax` (s), 0 for none.
    max: f32,
    /// The controller's skill list (lower-case): the parents, else itself.
    skills: Vec<String>,
}

/// A running buff's `refreshDurationMax` budget: what each source spent.
#[derive(Clone, Debug)]
struct Budget {
    /// The buff's entry name in `Loadout::buffs`.
    buff: String,
    /// Its time left after the last extension (a recast makes it jump up).
    seen: f32,
    spent: Vec<(String, f32)>,
}

/// The player's refresh controllers and the budgets of the buffs they
/// lengthened.
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
    data: Res<crate::data::GameData>,
    // Option: without an inventory only the worn items' refreshers are skipped.
    inv: Option<Res<crate::inventory::Inventory>>,
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
    // Modifiers (learned and gear) with the skills they modify: the links
    // key each wrapper under its profile name too.
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
    // Skills granted by worn items (the barrelsmith guns' pool attacks):
    // their own list is themselves.
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
    // The running buffs keep their budgets.
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
    data: Res<crate::data::GameData>,
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
                    .unwrap_or(crate::casting::DEFAULT_ATTACK)
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
            if r.chance > 0.0 && crate::rng::u32(0..=100) as f32 > r.chance {
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
            let mut add = r.amount.max(0.0);
            // The source's budget: attacks only.
            if let Some(src) = ev
                .used
                .as_deref()
                .filter(|_| matches!(ev.trigger, Trigger::AttackEnemy | Trigger::AttackEnemyCrit))
            {
                let src = src.to_ascii_lowercase();
                let own = if r.target.contains(&src) {
                    data.db.get(&r.target[0]).map_or(0.0, |t| {
                        level_f(
                            t,
                            "refreshDurationMax",
                            effective
                                .map_or(1, |e| loadout.skill_level(&data.db, e, &r.target[0]))
                                .max(1) as usize
                                - 1,
                        )
                    })
                } else {
                    0.0
                };
                let total = own
                    + refreshers
                        .list
                        .iter()
                        .filter(|o| o.duration && o.target == r.target && o.skills.contains(&src))
                        .map(|o| o.max)
                        .sum::<f32>();
                if total > 0.0 {
                    let b = match refreshers
                        .budgets
                        .iter()
                        .position(|b| b.buff.eq_ignore_ascii_case(&name))
                    {
                        Some(j) => j,
                        None => {
                            refreshers.budgets.push(Budget {
                                buff: name.clone(),
                                seen: loadout.buffs.list[i].2,
                                spent: Vec::new(),
                            });
                            refreshers.budgets.len() - 1
                        }
                    };
                    let spent = &mut refreshers.budgets[b].spent;
                    let j = spent.iter().position(|s| s.0 == src).unwrap_or_else(|| {
                        spent.push((src.clone(), 0.0));
                        spent.len() - 1
                    });
                    add = add.min(total - spent[j].1).max(0.0);
                    spent[j].1 += add;
                }
            }
            if add <= 0.0 {
                continue;
            }
            loadout.buffs.list[i].2 += add;
            if let Some(b) = refreshers
                .budgets
                .iter_mut()
                .find(|b| b.buff.eq_ignore_ascii_case(&name))
            {
                b.seen = loadout.buffs.list[i].2;
            }
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
