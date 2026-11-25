//! The character sheet's Damage Per Second and its tooltip lines: the weapon
//! attack's DPS, the left-mouse and right-mouse skills' DPS
//! (`GrimDawn.exe` `CharacterWindow::UpdatePage1` → `Player::CalculateSkillDps`:
//! the number shown is the left-mouse skill's, rounded; the rollover lists
//! the weapon attack and both skills, `tagCharStatsDPSWeapon`). The formulas
//! are `gd_sim::dps`.
// The sheet's accumulator counts a global-chance "one of" group (Elemental
// Infusion's 34/33/33 % cold / fire / lightning burn) as its first attribute
// only (`gd_sim::actor::GlobalGroup`, `DamageAttributeStore::
// AddDamageToAccumulator`), and a min-only flat next to a ranged one sums to
// a range (`fill_max`). The Elemental modifier raises cold / fire / lightning
// (the Elemental thirds too), and the skill's own modifiers travel with
// converted damage scaled by its share (`gd_sim::combat::convert_damage`).
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

/// What the sheet shows: DPS values plus the display-name tags of the two
/// mouse skills.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SheetDps {
    pub weapon: f32,
    /// Damage per hit (min, max) of the weapon attack: the "Weapon Damage"
    /// row of the Damage Per Hit block.
    pub weapon_hit: (f32, f32),
    /// The weapon attack's main-hand and off-hand damage per hit (the two
    /// accumulators of `CalculateSkillDps`, `GetMinMaxDamage`; the off-hand
    /// zero without a dual wield): the Weapon Damage rollover
    /// (`gd_sim::sheet::numbers_and_rollovers`, `CharacterWindow::UpdatePage2`).
    pub weapon_hands: [(f32, f32); 2],
    /// The sheet's "Attacks per Second" (`CharacterWindow::UpdatePage2`, see [`attacks_per_second`]).
    pub attacks_per_second: f32,
    pub left: Option<(String, f32)>,
    pub right: Option<(String, f32)>,
    /// Damage per hit (min, max) of the left / right mouse skill.
    pub hits: [(f32, f32); 2],
    /// The left / right mouse skill's damage lines (duration, type, min,
    /// max): the page II LMB / RMB rollovers, the skill's learned secondary
    /// skills merged in (`gd_sim::actor::display_breakdown_with_secondaries`).
    pub breakdown: [Vec<(bool, gd_sim::attributes::CombatType, f32, f32)>; 2],
}

impl SheetDps {
    /// The Damage Per Hit text of a (min, max) pair (`CharacterWindow::UpdatePage2`
    /// @ the skill rows: both rounded `(int)(x + 0.5)` (the max pulled down
    /// by one when the rounding overshoots a negative), `tagFormatDash`,
    /// replaced by `IntFormat` of the minimum when min equals max or is 0).
    pub fn hit_text((min, max): (f32, f32)) -> String {
        let (a, b) = ((min + 0.5).floor() as i64, (max + 0.5).floor() as i64);
        if a == b || a == 0 {
            format!("{a}")
        } else {
            format!("{a} - {b}")
        }
    }

    /// The sheet number: the left-mouse skill's DPS (the weapon attack's when
    /// the slot is empty), rounded like the engine (`floor(x + 0.5)`).
    pub fn number(&self) -> String {
        format!(
            "{:.0}",
            (self.left.as_ref().map_or(self.weapon, |l| l.1) + 0.5).floor()
        )
    }

    /// The DPS rollover window's numbers (`charinfo_statsdpsrolloverstyle.dbr`, a `rolloverline5.tpl`): line 2 the weapon
    /// attack's DPS, 3 / 4 the left / right mouse skill's with their names, each rounded like the engine's `IntFormat`
    /// (`floor(x + 0.5)`); line 1 (title) and 5 (description) carry no number.
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

/// Everything [`calculate`] reads.
pub struct Inputs<'a> {
    pub data: &'a GameData,
    pub rules: &'a Rules,
    pub profile: &'a Profile,
    pub loadout: &'a Loadout,
    /// The player's effective skills ([`gd_game_core::player::EffectiveSkills`]) for `loadout`.
    pub effective: &'a gd_sim::skills::SkillBook,
    pub caster: &'a Caster,
    pub links: Option<&'a SkillLinks>,
    /// The player's record and animation set (the weapons' hand state).
    pub record: &'a str,
    pub anims: &'a gd_sim::anim::AnimSet,
}

/// Frame count and frame rate of an animation file, read through the shared `.anm` store
/// ([`gd_game_core::character::AnimCache`], which gd-render's model cache shares: one parse per file).
fn frames(
    data: &GameData,
    cache: &gd_game_core::character::SharedAnims,
    path: &str,
) -> Option<(u32, u32)> {
    let a = gd_game_core::character::shared_anim(cache, data, path)?;
    Some((a.num_frames, a.frame_rate))
}

/// `Player::CalculateAttackSpeed(1)` / the weapon set's attack animation
/// duration (`CalculateSkillDps`, `CharacterWindow::UpdatePage2`): [`weapon_rate`] of
/// the capped attack speed, the character's base attack speed and the mean
/// duration of the worn set's Attack animations.
fn weapon_attack_rate(i: &Inputs, cache: &gd_game_core::character::SharedAnims) -> f32 {
    let pc = i.data.db.get(i.record);
    let base = pc.as_ref().map_or(1.0, |r| {
        r.f32("characterAttackSpeed").unwrap_or(1.0)
            * (1.0 + r.f32("characterAttackSpeedModifier").unwrap_or(0.0) / 100.0)
    });
    let dur = attack_anim_duration(i.data, i.anims, cache);
    weapon_rate(i.profile.speeds[1], base, dur)
}

/// The mean duration of the weapon set's Attack animations (frames / fps /
/// record speed, the variants' mean; [`weapon_rate`]).
fn attack_anim_duration(
    data: &GameData,
    set: &gd_sim::anim::AnimSet,
    cache: &gd_game_core::character::SharedAnims,
) -> f32 {
    set.slot(AnimType::Attack)
        .mean_duration(|p| frames(data, cache, p))
}

/// The weapon animation's rate for the item tooltip's base attack speed line
/// (`CharAttributeVal_BaseAttackSpeed::CreateText`
/// multiplies by the weapon animation's `GetFrameRate / GetLength`, both
/// virtuals of the `Item.animationSet` subobject): the reciprocal of the
/// Attack slot's mean duration. PORT: the mean of the variants with the record
/// speed folded in ([`weapon_attack_rate`]'s duration), the engine the selected
/// variant's raw rate; 1 without an animation (the engine leaves the factor).
pub fn weapon_anim_rate(
    data: &GameData,
    anims: &gd_sim::anim::AnimSet,
    cache: &gd_game_core::character::SharedAnims,
) -> f32 {
    let dur = attack_anim_duration(data, anims, cache);
    if dur > 0.0 { 1.0 / dur } else { 1.0 }
}

/// The sheet's "Attacks per Second" (`CharacterWindow::UpdatePage2`):
/// `CalculateAttackSpeed(1) × 0.01 / UnarmedAnimSpeed`, then −5 %. With a
/// weapon equipped `CalculateAttackSpeed` multiplies in
/// unarmed / weapon animation duration, which is [`weapon_attack_rate`].
pub fn attacks_per_second(i: &Inputs, cache: &gd_game_core::character::SharedAnims) -> f32 {
    let r = weapon_attack_rate(i, cache);
    r + r * -0.05
}

/// [`skill_dps`]'s values: the skill's display-name tag, its DPS, its damage per hit (min, max), the two hands' damage
/// per hit and its damage lines.
type SkillValues = (
    String,
    f32,
    (f32, f32),
    [(f32, f32); 2],
    Vec<(bool, gd_sim::attributes::CombatType, f32, f32)>,
);

/// `Player::CalculateSkillDps` for one hotbar skill (the default attack when
/// it cannot be used with the worn weapons or is unknown).
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
    // The rollover's secondaries are the hotbar skill's own, even when its
    // damage falls back to the default attack (`CharacterWindow::BuildSkillDamageRollover`:
    // `GetSecondarySkills` of the slot's skill object; the learned list only,
    // `SkillLinks::secondaries`).
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
    // `GetTotalSkillAttribute(skillCooldownReduction)`: the skill's own
    // (record + modifiers: value + |value| × modifier %) plus the
    // character's.
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

/// The sheet's DPS values for the player.
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

/// The values the sheet and its rollover read: a deliberate throttled cache
/// of derived values ([`update`] refreshes it at most twice a second, the
/// engine's character window rate).
#[derive(bevy::prelude::Resource, Default)]
pub struct SheetDpsValues(pub SheetDps);

/// [`update`]'s throttle and the inputs it last computed from.
#[derive(Default)]
pub struct UpdateState {
    /// Seconds until the next recompute is allowed.
    wait: f32,
    /// The profile `Arc` last seen (its address).
    profile: usize,
    slots: Vec<Option<String>>,
    dirty: bool,
}

/// Recomputes the values when the profile, hotbar, skills or gear changed
/// (at most twice a second, the rate the character window refreshes its
/// numbers at, `CharacterWindow::UpdatePage2`), once the combat rules are loaded.
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

/// The primary (left-mouse) skill's DPS, or the weapon attack's without one:
/// `Player::CalculateDps(skill)` of the primary hot slot.
pub fn primary_dps(i: &Inputs, cache: &gd_game_core::character::SharedAnims) -> f32 {
    i.caster.slots[SLOT_LEFT]
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| skill_dps(i, cache, s))
        .or_else(|| skill_dps(i, cache, DEFAULT_ATTACK))
        .map_or(0.0, |v| v.1)
}

/// The DPS change shown by `GetUIDisplayText_DPS`
/// (`Player::CompareItems`: the hovered weapon is equipped into the
/// main hand, `Player::CalculateDps` of the primary hot slot's skill runs
/// before / after, each rounded half up and subtracted; the no-Ctrl value
/// (`Player.compareDpsNoCtrl`) is taken with the components and augments of the compared items
/// removed, the Ctrl value (`Player.compareDpsCtrl`) with them, so `with_components` is the
/// Ctrl state). The profile, the animation set and the skill links
/// ([`crate::weapon_pool::build_links`]: modifiers, weapon pool, overrides)
/// are rebuilt from the tried gear, as the engine's equipped probe has them.
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
