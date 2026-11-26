//! Weapon-pool attacks and skill-modifier links of the player (`gd_sim::weapon_pool`, `gd_sim::actor::SkillUse`).
//!
//! [`SkillLinks`] keeps the learned modifiers per skill (their damage adds to every hit of the skill) and the
//! usable pool skills (`Skill_WPAttack*` with a chance weight, weapon requirements met). Each weapon-pool attack
//! (the default attack or a `Skill_WeaponPool_*` cast) rolls [`gd_sim::weapon_pool::select`]; a picked pool skill
//! fires instead as a [`gd_game_core::casting::ProcCast`] with the pool attack as its parent. The roll happens as
//! the action starts (`SkillActivatedWeaponPool::StartAction` → `SkillManager::SelectWeaponPoolSkill`):
//! [`start_swing`] for a plain swing and [`pool_cast_starts`] for a pool-skill cast store it in [`PoolPick`] until
//! the action's hit uses it. The picked skill's `skillSpecialAnimationName` plays as the swing's clip
//! ([`pool_swing_specials`]), and the hit point is that clip's `RightHandHit`. Shapes per class: [`pool_kind`].
//!
//! Monsters run the same code: their `Skill_WPAttack*` skills form their pool and a `Skill_WeaponPool_Charged*`
//! attack keeps [`PoolCharges`]. PORT: the monsters' pool skills' weapon requirements are not checked.
//!
//! Charged pool skills ([`PoolCharges`]):
//! - `Skill_WeaponPool_ChargedFinale` (Cadence): an attack with targets gains a charge (up to `skillChargeLevel`)
//!   unless locked out; the lockout is `skillChargeTime` (`…Ranged` with a ranged weapon) / attack speed, the
//!   charges expire after `skillChargeDuration`. With full charges and the lockout over the next attack is the
//!   finale, decided as it starts: the skill's own hit, no pool roll; a landed finale (or any with
//!   `spendChargesOnMiss`) clears the charges. Other attacks are plain pool attacks without the skill's damage
//!   (`Skill.isDamageParent` cleared).
//! - `Skill_WeaponPool_ChargedScaling` (Righteous Fervor): an attack with targets and the lockout over gains a
//!   charge (up to `skillChargeLevel`) before it hits; each charge lasts `skillChargeDuration`, then one is lost;
//!   the skill's own damage and weapon % scale by `skillChargeMultipliers[charges-1]` % (none without charges).
//!   Lockout = the profile's `skillChargeTime` (never the ranged one) / attack speed.
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_game_core::casting::{Casting, Kind, level_f};
use gd_game_core::character::{CharState, Character};
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player};
use gd_game_core::weapon_pool::{Charge, PoolCharges, SkillLinks, is_pool_parent};
use gd_rules::Rng;
use gd_sim::equip::WeaponType;

pub struct WeaponPoolPlugin;

impl Plugin for WeaponPoolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PoolRng>().add_systems(
            Update,
            (
                rebuild_links.run_if(resource_exists::<gd_game_core::inventory::Inventory>),
                toggles_need_weapons.after(rebuild_links),
                tick_charges,
                charged_profiles.after(tick_charges),
                pool_cast_starts.before(crate::casting::CastingSet),
                // The swing's clip is picked before the drive plays it.
                pool_swing_specials
                    .in_set(gd_game_core::character::CharacterStage::Drive)
                    .before(crate::character::drive_characters),
            ),
        );
    }
}

/// A toggled skill switches off when the worn weapons stop suiting it (`Skill_BuffSelfToggled::Update`); the auto
/// toggle switches it on again once they do.
fn toggles_need_weapons(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<(Entity, &SkillLinks, &mut Loadout), Changed<SkillLinks>>,
) {
    for (e, links, mut loadout) in &mut q {
        let off: Vec<String> = loadout
            .skills
            .skills
            .iter()
            .filter(|(s, l)| *l > 0 && !loadout.skills.switched_off(&s.to_ascii_lowercase()))
            .filter_map(|(s, _)| {
                data.db
                    .get(s)
                    .filter(|r| gd_sim::skills::is_toggle(&r.class))
                    .map(|r| (s, r))
            })
            .filter(|(s, r)| !links.weapons_allow(&data.db, r, s))
            .map(|(s, _)| s.to_ascii_lowercase())
            .collect();
        if off.is_empty() {
            continue;
        }
        for s in off {
            info!(
                "{} switched off: needs other weapons",
                s.rsplit('/').next().unwrap_or("")
            );
            loadout.skills.inactive.insert(s);
        }
        loadout.skills.touched();
        commands
            .entity(e)
            .try_insert(gd_game_core::player::ProfileDirty);
    }
}

#[derive(Resource)]
pub struct PoolRng(pub Rng);

impl Default for PoolRng {
    fn default() -> Self {
        Self(Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF)))
    }
}

/// The cast replacing a weapon-pool attack with the picked pool skill
/// (`parent`: the pool skill cast, `None` for the default attack).
pub fn pick_cast(
    data: &GameData,
    (skill, level_index): (String, usize),
    parent: Option<(String, usize)>,
    point: Vec3,
    target: Option<Entity>,
    hostile: bool,
) -> Option<Casting> {
    let kind = pool_kind(data.db.get(&skill)?);
    debug!(
        "weapon pool: {} (parent {:?})",
        skill.rsplit('/').next().unwrap_or(""),
        parent
            .as_ref()
            .map(|p| p.0.rsplit('/').next().unwrap_or(""))
    );
    Some(Casting {
        skill,
        level_index,
        kind,
        point,
        target,
        hostile,
        parent,
    })
}

/// A weapon-pool action's outcome, decided as it starts (`SelectWeaponPoolSkill`, or Cadence's finale when charged)
/// and used by the action's hits.
#[derive(Component, Clone, Debug, Default)]
pub struct PoolPick {
    /// The action's skill: [`gd_game_core::casting::DEFAULT_ATTACK`] for a plain
    /// swing, else the pool skill cast.
    pub skill: String,
    /// The pool skill picked to fire instead (record, level index).
    pub pick: Option<(String, usize)>,
    /// Cadence's finale (no roll).
    pub finale: bool,
}

impl PoolPick {
    pub fn is_for(&self, skill: &str) -> bool {
        self.skill.eq_ignore_ascii_case(skill)
    }
}

/// A plain weapon swing (or shot) starts: roll the pool for it.
pub fn start_swing(
    commands: &mut Commands,
    entity: Entity,
    links: Option<&SkillLinks>,
    rng: &mut Rng,
) {
    let pick = links.and_then(|l| l.roll(rng));
    commands.entity(entity).try_insert(PoolPick {
        skill: gd_game_core::casting::DEFAULT_ATTACK.into(),
        pick,
        finale: false,
    });
}

/// A weapon-pool proc's swing plays the picked skill's `skillSpecialAnimationName` ([`Character::swing_special`]);
/// the hit point is that clip's `RightHandHit`. Runs as the swing starts, after [`PoolPick`] is inserted and before
/// the drive plays its clip; a pick without a special name keeps the plain clip.
fn pool_swing_specials(
    data: Res<GameData>,
    mut q: Query<(&PoolPick, &mut Character), Changed<PoolPick>>,
) {
    for (p, mut ch) in &mut q {
        // Pool-skill casts (Fire Strike itself…) animate through their Cast
        // state; only a plain swing's roll replaces the attack clip.
        if ch.state != CharState::Attack {
            continue;
        }
        ch.swing_special = p.pick.as_ref().and_then(|(skill, _)| {
            data.db
                .get(skill)
                .and_then(|r| r.str("skillSpecialAnimationName").map(str::to_owned))
        });
    }
}

/// A pool skill's action starts: the finale when Cadence is charged, else
/// a roll.
pub fn start_pool_action(
    links: &SkillLinks,
    charges: &PoolCharges,
    rec: &Record,
    skill: &str,
    level_index: usize,
    rng: &mut Rng,
) -> PoolPick {
    let finale = charges.finale_ready(rec, skill, level_index);
    PoolPick {
        skill: skill.to_owned(),
        pick: if finale { None } else { links.roll(rng) },
        finale,
    }
}

/// Pool-skill casts (Fire Strike, Cadence, Righteous Fervor) starting their
/// animation decide their pool outcome now ([`start_pool_action`]).
fn pool_cast_starts(
    mut commands: Commands,
    data: Res<GameData>,
    mut rng: ResMut<PoolRng>,
    q: Query<(Entity, &Casting, &SkillLinks, &PoolCharges), Added<Casting>>,
) {
    for (e, cast, links, charges) in &q {
        if cast.kind != Kind::Weapon || cast.parent.is_some() {
            continue;
        }
        let Some(rec) = data.db.get(&cast.skill).filter(|r| is_pool_parent(r)) else {
            continue;
        };
        commands.entity(e).try_insert(start_pool_action(
            links,
            charges,
            rec,
            &cast.skill,
            cast.level_index,
            &mut rng.0,
        ));
    }
}

/// How a pool skill hits, by class: `…_Radius` hits the foes around the target (`skillTargetRadius`),
/// `…_AttackWave` sends a wave (`waveDistance`), `…_ProjectileBurst` fires `projectileLaunchNumber` projectiles;
/// `…_BasicAttack` and `…_RadialCrit` keep the weapon hit's targeting (the target plus `skillTargetNumber` foes in
/// `skillTargetAngle`).
pub fn pool_kind(rec: &Record) -> Kind {
    match rec.class.as_str() {
        "Skill_WPAttack_Radius" => Kind::TargetRadius,
        "Skill_WPAttack_AttackWave" => Kind::Wave,
        "Skill_WPAttack_ProjectileBurst" => Kind::Projectile,
        _ => Kind::Weapon,
    }
}

/// A charged `Skill_WeaponPool_ChargedScaling` adds its passive part to the player
/// (`gd_sim::skills::charged_scaling_passive`): the profile is rebuilt when the set of charged ones changes.
fn charged_profiles(
    mut commands: Commands,
    data: Res<GameData>,
    q: Query<(Entity, &PoolCharges, &gd_game_core::combat::Combatant), With<Player>>,
) {
    for (e, charges, cb) in &q {
        let mut now: Vec<&str> = charges
            .0
            .iter()
            .filter(|(s, c)| {
                c.count > 0
                    && data
                        .db
                        .get(s)
                        .is_some_and(|r| r.class == "Skill_WeaponPool_ChargedScaling")
            })
            .map(|(s, _)| s.as_str())
            .collect();
        now.sort_unstable();
        let mut built: Vec<String> = cb
            .profile
            .charged
            .iter()
            .map(|c| c.0.to_ascii_lowercase())
            .collect();
        built.sort_unstable();
        if now != built {
            commands
                .entity(e)
                .try_insert(gd_game_core::player::ProfileDirty);
        }
    }
}

fn tick_charges(
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<&mut PoolCharges>,
    mut landed: MessageReader<crate::combat::HitLanded>,
) {
    let dt = time.delta_secs();
    for h in landed.read() {
        if let (Ok(mut pc), Some(s)) = (q.get_mut(h.attacker), &h.skill)
            && data
                .db
                .get(s)
                .is_some_and(|r| r.class == "Skill_WeaponPool_ChargedFinale")
            && let Some(c) = pc.0.get_mut(&s.to_ascii_lowercase())
        {
            *c = Charge::default();
        }
    }
    for mut pc in &mut q {
        for (skill, c) in pc.0.iter_mut() {
            c.lockout -= dt;
            if c.count == 0 {
                continue;
            }
            c.decay -= dt;
            if c.decay > 0.0 {
                continue;
            }
            let Some(rec) = data.db.get(skill) else {
                continue;
            };
            if rec.class == "Skill_WeaponPool_ChargedScaling" {
                c.count -= 1;
                c.decay = if c.count > 0 {
                    level_f(rec, "skillChargeDuration", c.count as usize - 1)
                } else {
                    0.0
                };
            } else {
                c.count = 0;
            }
        }
    }
}

/// The [`SkillLinks`] of a loadout with the skill modifiers of the worn items `gear` and the weapon types `hands`
/// (right, left); also built for the tooltip's DPS compare (`Player::CompareItems` equips the candidate, so its
/// modifiers count and the replaced item's do not).
pub fn build_links(
    data: &GameData,
    trees: &gd_sim::skills::SkillTrees,
    loadout: &Loadout,
    gear: &[gd_formats::save::item::Item],
    hands: (Option<WeaponType>, Option<WeaponType>),
) -> SkillLinks {
    let book = loadout.effective_skills(&data.db, trees);
    let (right, left) = hands;
    let mut links = SkillLinks::default();
    // Pool skills: (record, level index, learned modifiers), qualified once
    // the gear modifiers are known.
    let mut pool_candidates: Vec<(String, usize, Vec<(String, usize)>)> = Vec::new();
    links.hands = (right, left);
    // Lower-case skill → its learned modifiers (to tell them from gear ones).
    let mut learned_of: HashMap<String, Vec<String>> = HashMap::default();
    for (s, l) in &book.skills {
        if *l == 0
            || gd_sim::skills::is_modifier_class(
                &data.db.get(s).map(|r| r.class.clone()).unwrap_or_default(),
            )
        {
            continue;
        }
        let mods = book.modifiers_of(&data.db, trees, s);
        let Some(rec) = data.db.get(s) else { continue };
        let li = book.effective_level(&data.db, s).max(1) as usize - 1;
        if rec.class.starts_with("Skill_WPAttack") {
            pool_candidates.push((s.clone(), li, mods.clone()));
        }
        learned_of.insert(
            s.to_ascii_lowercase(),
            mods.iter().map(|(m, _)| m.to_ascii_lowercase()).collect(),
        );
        let (sec, mods): (Vec<_>, Vec<_>) = mods.into_iter().partition(|(m, _)| {
            data.db
                .get(m)
                .is_some_and(|r| gd_sim::skills::is_secondary_class(&r.class))
        });
        // Wrapper skills cast (and hit) as their `buffSkillName` record:
        // key the links under both names.
        let mut keys = vec![s.to_ascii_lowercase()];
        if let Some(b) = gd_sim::skills::profile_name(rec) {
            keys.push(b.to_ascii_lowercase());
        }
        for k in keys {
            if !mods.is_empty() {
                links.mods.insert(k.clone(), mods.clone());
            }
            if !sec.is_empty() {
                links.secondaries.insert(k, sec.clone());
            }
        }
    }
    // Gear skill modifiers (`modifiedSkillName{n}` → `modifierSkillName{n}`) join the learned ones at level index 0
    // (`SkillManager::AddItemSkillModifiers`, once each for the item, its component and its augment). A modifier of
    // skill S whose call also modifies a `Skill_Transmuter` of S held by S's learned modifiers is exclusive to it
    // and off while that transmuter is enabled (`SetExclusiveItemSkillModifier` / `IsSkillEnabled`). An off
    // modifier adds nothing and has no target effect, so it is kept apart ([`SkillLinks::off_mods`]): only
    // `Skill::CreateWarmUpFx` runs it.
    for it in gear {
        for names in [
            &[
                &it.base_name,
                &it.prefix_name,
                &it.suffix_name,
                &it.modifier_name,
                &it.ascendant_name,
            ][..],
            &[&it.relic_name, &it.relic_bonus][..],
            &[&it.augment_name][..],
        ] {
            let pairs: Vec<(String, String)> = names
                .iter()
                .filter(|n| !n.is_empty())
                .filter_map(|n| data.db.get(n.as_str()))
                .flat_map(|r| {
                    (1..=8)
                        .filter_map(|n| {
                            let target = r.str(&format!("modifiedSkillName{n}"))?;
                            let m = r.str(&format!("modifierSkillName{n}"))?;
                            (!target.is_empty() && !m.is_empty())
                                .then(|| (target.to_owned(), m.to_owned()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            for (target, m) in &pairs {
                let secondary = data
                    .db
                    .get(m)
                    .is_some_and(|mr| gd_sim::skills::is_secondary_class(&mr.class));
                let learned = learned_of.get(&target.to_ascii_lowercase());
                let off = !secondary
                    && pairs.iter().any(|(t, _)| {
                        !t.eq_ignore_ascii_case(target)
                            && learned.is_some_and(|l| l.contains(&t.to_ascii_lowercase()))
                            && data
                                .db
                                .get(t)
                                .is_some_and(|tr| tr.class == "Skill_Transmuter")
                    });
                let mut keys = vec![target.to_ascii_lowercase()];
                if let Some(b) = data
                    .db
                    .get(target)
                    .and_then(|tr| gd_sim::skills::profile_name(tr).map(str::to_ascii_lowercase))
                {
                    keys.push(b);
                }
                for k in keys {
                    let map = if secondary {
                        &mut links.item_secondaries
                    } else if off {
                        &mut links.off_mods
                    } else {
                        &mut links.mods
                    };
                    map.entry(k).or_default().push((m.clone(), 0));
                }
            }
        }
    }
    // Weapon requirement overrides (`overwriteBaseSkill`): first learned
    // modifier, else the last gear one.
    for (k, list) in &links.mods {
        let learned = learned_of.get(k).cloned().unwrap_or_default();
        let recs: Vec<(Record, bool)> = list
            .iter()
            .filter_map(|(m, _)| {
                data.db
                    .get_owned(m)
                    .map(|r| (r, learned.contains(&m.to_ascii_lowercase())))
            })
            .collect();
        let l: Vec<&Record> = recs.iter().filter(|(_, x)| *x).map(|(r, _)| r).collect();
        let g: Vec<&Record> = recs.iter().filter(|(_, x)| !*x).map(|(r, _)| r).collect();
        if let Some(o) = gd_sim::weapon_pool::weapon_list_override(&l, &g) {
            links.weapon_override.insert(k.clone(), o.name.clone());
        }
    }
    for (s, li, mods) in pool_candidates {
        let Some(rec) = data.db.get(&s) else { continue };
        let mrecs: Vec<(Record, usize)> = mods
            .iter()
            .filter_map(|(m, ml)| data.db.get_owned(m).map(|r| (r, *ml)))
            .collect();
        let gear: Vec<Record> = links
            .mods_of(&s)
            .iter()
            .filter(|(m, _)| !mods.iter().any(|(l, _)| l.eq_ignore_ascii_case(m)))
            .filter_map(|(m, _)| data.db.get_owned(m))
            .collect();
        let learned: Vec<&Record> = mrecs.iter().map(|(r, _)| r).collect();
        let over =
            gd_sim::weapon_pool::weapon_list_override(&learned, &gear.iter().collect::<Vec<_>>());
        if !gd_sim::weapon_pool::qualifies(rec, over, right, left) {
            continue;
        }
        let mrefs: Vec<(&Record, usize)> = mrecs.iter().map(|(r, l)| (r, *l)).collect();
        let w = gd_sim::weapon_pool::chance_weight(rec, li, &mrefs);
        if w > 0 {
            links.pool.push((s, li, w));
        }
    }
    links
}

fn rebuild_links(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    data: Res<GameData>,
    inv: Res<gd_game_core::inventory::Inventory>,
    players: Query<(Entity, Ref<Loadout>), With<Player>>,
    mut sig: Local<u64>,
) {
    if fresh.begun() {
        *sig = Default::default();
    }
    use std::hash::{Hash, Hasher};
    let Ok((e, loadout)) = players.single() else {
        return;
    };
    // Only actual mutations wake this (UI reads don't tick the resource); the signature hash below dedups.
    if !loadout.is_changed() && !inv.is_changed() {
        return;
    }
    // Only skills and gear matter (the loadout also changes with buffs).
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        e,
        &loadout.skills.skills,
        loadout
            .items
            .iter()
            .map(|i| {
                (
                    &i.base_name,
                    &i.prefix_name,
                    &i.suffix_name,
                    &i.relic_name,
                    &i.augment_name,
                )
            })
            .collect::<Vec<_>>(),
    )
        .hash(&mut h);
    // Detached (requirement-failing) gear grants nothing: hash, gear and
    // weapon types all come from attached slots only.
    let attached = |i: usize| {
        inv.is_attached(i)
            .then(|| inv.equipped[i].as_ref())
            .flatten()
    };
    (0..inv.equipped.len())
        .map(|i| {
            attached(i).map(|i| {
                (
                    &i.base_name,
                    &i.prefix_name,
                    &i.suffix_name,
                    &i.modifier_name,
                    &i.relic_name,
                    &i.augment_name,
                )
            })
        })
        .collect::<Vec<_>>()
        .hash(&mut h);
    if std::mem::replace(&mut *sig, h.finish()) == *sig {
        return;
    }
    let trees = data.skill_trees();
    let wtype = |i: usize| {
        attached(i)
            .and_then(|it| data.db.get(&it.base_name))
            .and_then(|r| WeaponType::from_class(&r.class))
    };
    let gear = inv.worn();
    let links = build_links(&data, &trees, &loadout, &gear, (wtype(12), wtype(13)));
    debug!(
        "skill links: pool {:?}, {} modified skills",
        links
            .pool
            .iter()
            .map(|p| (p.0.rsplit('/').next().unwrap_or(""), p.2))
            .collect::<Vec<_>>(),
        links.mods.len()
    );
    commands.entity(e).try_insert(links);
    commands.entity(e).try_insert_if_new(PoolCharges::default());
}
