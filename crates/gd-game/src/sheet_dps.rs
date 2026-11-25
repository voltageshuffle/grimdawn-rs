//! The character sheet's Damage Per Second and its tooltip lines (`CharacterWindow::UpdatePage1` →
//! `Player::CalculateSkillDps`): the number is the left-mouse skill's, rounded; the rollover lists
//! the weapon attack and both mouse skills. The formulas are `gd_sim::dps`.
// The accumulator counts a global-chance "one of" group (Elemental Infusion's cold / fire /
// lightning burn) as its first attribute only (`DamageAttributeStore::AddDamageToAccumulator`),
// and a min-only flat next to a ranged one sums to a range (`fill_max`).
use bevy::ecs::change_detection::DetectChanges;
use gd_formats::arz::Record;
use gd_game_core::casting::{Caster, DEFAULT_ATTACK, SLOT_LEFT, SLOT_RIGHT};
use gd_game_core::character::{Character, anim_set_for};
use gd_game_core::data::GameData;
use gd_game_core::player::Loadout;
use gd_game_core::sheet_dps::Gear;
use gd_game_core::weapon_pool::SkillLinks;
use gd_sim::actor::{Profile, Rules, SkillUse, display_damage, display_damage_hands};
use gd_sim::anim::{AnimType, skill_animation};
use gd_sim::dps::{Info, finish, spell_dps, spell_rate, weapon_dps, weapon_rate};
use gd_sim::equip::WeaponType;
use gd_sim::stats::level_value;

/// DPS values plus the display-name tags of the two mouse skills.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SheetDps {
    pub weapon: f32,
    /// The "Weapon Damage" row of the Damage Per Hit block.
    pub weapon_hit: (f32, f32),
    /// Main-hand and off-hand damage per hit (`CalculateSkillDps`'s two accumulators; off-hand zero
    /// without dual wield): the Weapon Damage rollover.
    pub weapon_hands: [(f32, f32); 2],
    /// See [`attacks_per_second`].
    pub attacks_per_second: f32,
    pub left: Option<(String, f32)>,
    pub right: Option<(String, f32)>,
    /// Damage per hit (min, max) of the left / right mouse skill.
    pub hits: [(f32, f32); 2],
    /// (duration, type, min, max) lines of the LMB / RMB rollovers, learned secondaries merged in.
    pub breakdown: [Vec<(bool, gd_sim::attributes::CombatType, f32, f32)>; 2],
}

impl SheetDps {
    /// `CharacterWindow::UpdatePage2`'s skill rows: both rounded `(int)(x + 0.5)`,
    /// `tagFormatDash`, or just the minimum when min equals max or is 0.
    pub fn hit_text((min, max): (f32, f32)) -> String {
        let (a, b) = ((min + 0.5).floor() as i64, (max + 0.5).floor() as i64);
        if a == b || a == 0 {
            format!("{a}")
        } else {
            format!("{a} - {b}")
        }
    }

    /// The left-mouse skill's DPS (the weapon attack's when the slot is empty), `floor(x + 0.5)`.
    pub fn number(&self) -> String {
        format!(
            "{:.0}",
            (self.left.as_ref().map_or(self.weapon, |l| l.1) + 0.5).floor()
        )
    }

    /// The DPS rollover's lines (`rolloverline5.tpl`): 2 the weapon attack's DPS, 3 / 4 the left /
    /// right mouse skill's with their names, each `floor(x + 0.5)`; 1 and 5 carry no number.
    pub fn roll_values(&self) -> gd_sim::sheet::RollValues {
        let round = |v: f32| (v + 0.5).floor();
        let mut r = gd_sim::sheet::RollValues::default();
        r.lines[1] = (round(self.weapon), 0.0);
        for (n, skill) in [(2, &self.left), (3, &self.right)] {
            if let Some((tag, dps)) = skill {
                r.lines[n] = (round(*dps), 0.0);
                r.labels[n] = Some(tag.clone());
            }
        }
        r
    }
}

pub struct Inputs<'a> {
    pub data: &'a GameData,
    pub rules: &'a Rules,
    pub profile: &'a Profile,
    pub loadout: &'a Loadout,
    /// The player's effective skills for `loadout`.
    pub effective: &'a gd_sim::skills::SkillBook,
    pub caster: &'a Caster,
    pub links: Option<&'a SkillLinks>,
    /// The player's record and animation set (the weapons' hand state).
    pub record: &'a str,
    pub anims: &'a gd_sim::anim::AnimSet,
}

/// Frame count and frame rate of an animation file, through the shared `.anm` store.
fn frames(
    data: &GameData,
    cache: &gd_game_core::character::SharedAnims,
    path: &str,
) -> Option<(u32, u32)> {
    let a = gd_game_core::character::shared_anim(cache, data, path)?;
    Some((a.num_frames, a.frame_rate))
}

/// `Player::CalculateAttackSpeed(1)` / the worn set's mean Attack animation duration.
fn weapon_attack_rate(i: &Inputs, cache: &gd_game_core::character::SharedAnims) -> f32 {
    let pc = i.data.db.get(i.record);
    let base = pc.as_ref().map_or(1.0, |r| {
        r.f32("characterAttackSpeed").unwrap_or(1.0)
            * (1.0 + r.f32("characterAttackSpeedModifier").unwrap_or(0.0) / 100.0)
    });
    let dur = attack_anim_duration(i.data, i.anims, cache);
    weapon_rate(i.profile.speeds[1], base, dur)
}

/// Frames / fps / record speed, averaged over the variants.
fn attack_anim_duration(
    data: &GameData,
    set: &gd_sim::anim::AnimSet,
    cache: &gd_game_core::character::SharedAnims,
) -> f32 {
    set.slot(AnimType::Attack)
        .mean_duration(|p| frames(data, cache, p))
}

/// The item tooltip's base attack speed factor (`CharAttributeVal_BaseAttackSpeed::CreateText`):
/// the reciprocal of the Attack slot's duration. PORT: the variants' mean with the record speed
/// folded in, where the engine uses the selected variant's raw rate; 1 without an animation.
pub fn weapon_anim_rate(
    data: &GameData,
    anims: &gd_sim::anim::AnimSet,
    cache: &gd_game_core::character::SharedAnims,
) -> f32 {
    let dur = attack_anim_duration(data, anims, cache);
    if dur > 0.0 { 1.0 / dur } else { 1.0 }
}

/// `CharacterWindow::UpdatePage2`: `CalculateAttackSpeed(1) × 0.01 / UnarmedAnimSpeed`, then −5 %;
/// with a weapon this is [`weapon_attack_rate`].
pub fn attacks_per_second(i: &Inputs, cache: &gd_game_core::character::SharedAnims) -> f32 {
    let r = weapon_attack_rate(i, cache);
    r + r * -0.05
}

/// Display-name tag, DPS, damage per hit (min, max), the two hands' damage per hit, damage lines.
type SkillValues = (
    String,
    f32,
    (f32, f32),
    [(f32, f32); 2],
    Vec<(bool, gd_sim::attributes::CombatType, f32, f32)>,
);

/// `Player::CalculateSkillDps` for one hotbar skill (the default attack when it cannot be used with
/// the worn weapons or is unknown).
fn skill_dps(
    i: &Inputs,
    cache: &gd_game_core::character::SharedAnims,
    skill: &str,
) -> Option<SkillValues> {
    let data = i.data;
    let usable = |s: &str| {
        data.db
            .get(s)
            .filter(|r| i.links.is_none_or(|l| l.weapons_allow(&data.db, r, s)))
    };
    // The rollover's secondaries are the hotbar skill's own learned ones, even when its damage
    // falls back to the default attack (`CharacterWindow::BuildSkillDamageRollover`).
    let secondary_recs: Vec<(Record, usize)> = i
        .links
        .and_then(|l| l.secondaries.get(&skill.to_ascii_lowercase()))
        .into_iter()
        .flatten()
        .filter_map(|(s, li)| data.db.get_owned(s).map(|r| (r, *li)))
        .collect();
    let level = usable(skill)
        .map(|r| (r, i.loadout.skill_level(&data.db, i.effective, skill)))
        .filter(|(_, l)| *l > 0);
    let (skill, rec, level) = match level {
        Some((r, l)) => (skill, r, l),
        _ => (
            DEFAULT_ATTACK,
            data.db.get(DEFAULT_ATTACK)?,
            i.loadout
                .skill_level(&data.db, i.effective, DEFAULT_ATTACK)
                .max(1),
        ),
    };
    let li = (level - 1) as usize;
    // Wrapper skills keep their data in `buffSkillName`.
    let data_rec = gd_sim::skills::profile_name(rec)
        .and_then(|n| data.db.get(n))
        .unwrap_or(rec);
    let mods: Vec<(Record, usize)> = i
        .links
        .map(|l| {
            l.mods_of(skill)
                .iter()
                .filter_map(|(m, ml)| data.db.get_owned(m).map(|r| (r, *ml)))
                .collect()
        })
        .unwrap_or_default();
    let use_for = || {
        let mut u = SkillUse::new(data_rec, li);
        u.mods = mods.iter().map(|(m, l)| (m, *l)).collect();
        u.hands = i.links.map(|l| l.hands);
        u
    };
    let (min1, max1) = display_damage(i.rules, i.profile, &Some(use_for()));
    let sum = |k: &str| {
        level_value(data_rec, k, li).unwrap_or(0.0)
            + mods
                .iter()
                .map(|(r, l)| level_value(r, k, *l).unwrap_or(0.0))
                .sum::<f32>()
    };
    let cooldown = sum("skillCooldownTime");
    // `GetTotalSkillAttribute(skillCooldownReduction)`: the skill's own (record + modifiers) plus
    // the character's.
    let (own, own_pct) = (
        sum("skillCooldownReduction"),
        sum("skillCooldownReductionModifier"),
    );
    let cdr =
        (own + own.abs() * own_pct * 0.01 + i.profile.skill_attribute("skillCooldownReduction"))
            * 0.01;
    let mut info = Info {
        min: [min1, 0.0],
        max: [max1, 0.0],
        cooldown,
        cooldown_reduction: cdr,
        active_duration: sum("skillActiveDuration"),
        ..Default::default()
    };
    let (anim, weapon) = skill_animation(&rec.class)?;
    let dual_off = (weapon
        && i.links
            .and_then(|l| l.hands.1)
            .is_some_and(|t| !matches!(t, WeaponType::Shield | WeaponType::Offhand)))
    .then(|| i.profile.off_hand())
    .flatten();
    let secondaries = secondary_recs
        .iter()
        .map(|(r, li)| {
            let mut u = SkillUse::new(r, *li);
            u.hands = i.links.map(|l| l.hands);
            u
        })
        .collect();
    let breakdown = gd_sim::actor::display_breakdown_with_secondaries(
        i.rules,
        i.profile,
        dual_off,
        &Some(use_for()),
        secondaries,
    );
    let set = i.anims;
    let speeds = i.profile.speeds;
    let dps = if weapon {
        info.dual = i
            .links
            .and_then(|l| l.hands.1)
            .is_some_and(|t| !matches!(t, WeaponType::Shield | WeaponType::Offhand));
        if info.dual
            && let Some(off) = dual_off
        {
            let ((a, b), (c, d)) = display_damage_hands(i.rules, i.profile, off, &Some(use_for()));
            (info.min, info.max) = ([a, c], [b, d]);
        }
        let rate = weapon_attack_rate(i, cache);
        info.rate = rate;
        weapon_dps(&info)
    } else {
        info.rate = if rec.class == "Skill_AttackRadiusSpin" {
            speeds[1]
        } else {
            0.0
        };
        if rec.class != "Skill_AttackRadiusSpin" {
            let special = i
                .loadout
                .skills
                .special_animation(&data.db, &data.skill_trees(), skill);
            let entry = special
                .and_then(|n| {
                    set.specials
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(&n))
                        .and_then(|(_, s)| s.entries.first())
                })
                .or_else(|| set.slot(anim).entries.first());
            if let Some((n, fps)) = entry.and_then(|e| frames(data, cache, &e.path)) {
                info.rate = spell_rate(speeds[2], n, fps).unwrap_or(0.0);
            }
        }
        spell_dps(&info)
    };
    tracing::debug!(
        "sheet dps {skill}: damage {min1:.0}-{max1:.0} off {:.0}-{:.0} rate {:.3} cooldown {cooldown} cdr {cdr} -> {dps:.1}",
        info.min[1],
        info.max[1],
        info.rate
    );
    let hands = [(info.min[0], info.max[0]), (info.min[1], info.max[1])];
    Some((
        rec.str("skillDisplayName").unwrap_or_default().to_owned(),
        finish(dps, cooldown, cdr),
        (min1, max1),
        hands,
        breakdown,
    ))
}

pub fn calculate(i: &Inputs, cache: &gd_game_core::character::SharedAnims) -> SheetDps {
    let w = skill_dps(i, cache, DEFAULT_ATTACK);
    let (weapon, weapon_hit, weapon_hands) =
        w.map_or((0.0, (0.0, 0.0), [(0.0, 0.0); 2]), |w| (w.1, w.2, w.3));
    let slot = |n: usize| i.caster.slots[n].as_deref().filter(|s| !s.is_empty());
    let one = |n: usize| slot(n).and_then(|s| skill_dps(i, cache, s));
    let split = |v: Option<SkillValues>| match v {
        Some((tag, dps, hit, _, lines)) => (Some((tag, dps)), hit, lines),
        None => (None, (0.0, 0.0), Vec::new()),
    };
    let ((left, left_hit, left_lines), (right, right_hit, right_lines)) =
        (split(one(SLOT_LEFT)), split(one(SLOT_RIGHT)));
    SheetDps {
        weapon,
        weapon_hit,
        weapon_hands,
        attacks_per_second: attacks_per_second(i, cache),
        left,
        right,
        hits: [left_hit, right_hit],
        breakdown: [left_lines, right_lines],
    }
}

/// Refreshed by [`update`] at most twice a second, the engine's character window rate.
#[derive(bevy::prelude::Resource, Default)]
pub struct SheetDpsValues(pub SheetDps);

#[derive(Default)]
pub struct UpdateState {
    wait: f32,
    /// The profile `Arc`'s address last seen.
    profile: usize,
    slots: Vec<Option<String>>,
    dirty: bool,
}

/// Recomputes when the profile, hotbar, skills or gear changed, at most twice a second, once the
/// combat rules are loaded.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut fresh: gd_game_core::session::NewSession,
    time: bevy::prelude::Res<bevy::prelude::Time>,
    data: bevy::prelude::Res<GameData>,
    rules: bevy::prelude::Res<gd_game_core::rules::CombatRules>,
    q: bevy::prelude::Query<
        (
            &gd_game_core::combat::Combatant,
            bevy::prelude::Ref<Loadout>,
            bevy::prelude::Ref<gd_game_core::player::EffectiveSkills>,
            &Caster,
            Option<bevy::prelude::Ref<SkillLinks>>,
            &Character,
        ),
        bevy::prelude::With<gd_game_core::player::Player>,
    >,
    mut out: bevy::prelude::ResMut<SheetDpsValues>,
    mut state: bevy::prelude::Local<UpdateState>,
    cache: bevy::prelude::Res<gd_game_core::character::AnimCache>,
) {
    if fresh.begun() {
        *state = Default::default();
    }
    state.wait -= time.delta_secs();
    let Ok((cb, loadout, effective, caster, links, character)) = q.single() else {
        return;
    };
    let ptr = std::sync::Arc::as_ptr(&cb.profile) as usize;
    if loadout.is_changed()
        || effective.is_changed()
        || links.as_ref().is_some_and(|l| l.is_changed())
        || ptr != state.profile
        || caster.slots[..] != state.slots[..]
    {
        state.dirty = true;
    }
    if state.wait > 0.0 || !state.dirty {
        return;
    }
    state.wait = 0.5;
    state.profile = ptr;
    state.slots.clear();
    state.slots.extend_from_slice(&caster.slots);
    state.dirty = false;
    let v = calculate(
        &Inputs {
            data: &data,
            rules: &rules.0,
            profile: &cb.profile,
            loadout: &loadout,
            effective: &effective,
            caster,
            links: links.as_deref(),
            record: &character.record,
            anims: &character.anims,
        },
        &cache.shared(),
    );
    if v != out.0 {
        bevy::log::info!(
            "sheet DPS: weapon {:.0} left {:?} right {:?}",
            v.weapon,
            v.left,
            v.right
        );
        out.0 = v;
    }
}

pub struct SheetDpsPlugin;

impl bevy::prelude::Plugin for SheetDpsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        use bevy::prelude::{IntoScheduleConfigs, resource_exists};
        app.init_resource::<SheetDpsValues>().add_systems(
            bevy::prelude::Update,
            update
                .run_if(resource_exists::<GameData>)
                .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                .after(gd_game_core::player::PlayerSet),
        );
    }
}

/// `Player::CalculateDps` of the primary hot slot, or the weapon attack's without one.
pub fn primary_dps(i: &Inputs, cache: &gd_game_core::character::SharedAnims) -> f32 {
    i.caster.slots[SLOT_LEFT]
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| skill_dps(i, cache, s))
        .or_else(|| skill_dps(i, cache, DEFAULT_ATTACK))
        .map_or(0.0, |v| v.1)
}

/// `GetUIDisplayText_DPS` (`Player::CompareItems`): the hovered weapon equipped into the main hand,
/// `Player::CalculateDps` of the primary skill before / after, each rounded half up, subtracted.
/// `with_components`: the Ctrl state (no Ctrl strips the compared items' components and augments).
/// Profile, animation set and skill links are rebuilt from the tried gear.
pub fn compare_dps(
    i: &Inputs,
    cache: &gd_game_core::character::SharedAnims,
    now: &Gear,
    with: &Gear,
    with_components: bool,
) -> Option<f32> {
    let trees = i.data.skill_trees();
    let dps = |g: &Gear| -> Option<f32> {
        let data = i.data;
        let mut loadout = i.loadout.clone();
        loadout.items = g.items.clone();
        if !with_components {
            for it in &mut loadout.items {
                it.relic_name.clear();
                it.augment_name.clear();
            }
        }
        let effective = loadout.effective_skills(&data.db, &trees);
        let profile = loadout.profile(data, i.rules, i.profile.level)?;
        let rec = data.db.get(i.record)?;
        let anims = anim_set_for(
            data,
            rec,
            gd_sim::equip::HandState::from_weapons(g.hands.0, g.hands.1),
        );
        let links =
            crate::weapon_pool::build_links(data, &trees, &loadout, &loadout.items, g.hands);
        let v = primary_dps(
            &Inputs {
                profile: &profile,
                loadout: &loadout,
                effective: &effective,
                links: Some(&links),
                anims: &anims,
                ..*i
            },
            cache,
        );
        Some((v + 0.5).floor())
    };
    Some(dps(with)? - dps(now)?)
}
