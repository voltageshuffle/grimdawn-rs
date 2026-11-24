//! The player systems: profile rebuilds, effective skills and item toggles
//! (the player data — [`Player`], [`Loadout`] — is [`gd_game_core::player`]).
use bevy::prelude::*;
use gd_formats::save::gdc::Gdc;
use gd_game_core::combat::Combatant;
use gd_game_core::data::GameData;
use gd_game_core::player::{
    EffectiveSkills, ItemToggles, Loadout, Player, PlayerSet, ProfileDirty, SpawnData,
    player_record, skill_book_from_save,
};
use gd_game_core::progress::PlayerProgress;
use gd_game_core::rules::CombatRules;
use gd_sim::skills::SkillBook;

pub struct PlayerStatePlugin;

impl Plugin for PlayerStatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                rebuild_profile.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                derive_effective_skills,
                unequipped_toggles,
            )
                .after(PlayerSet),
        );
    }
}

/// The gameplay components of the player entity: the marker, its loadout
/// and its combat state (profile at `level`, full life, the player record's
/// `Actor::GetRadius`, `combat::get_radius`). The game adds the body, camera focus and movement path;
/// headless tests spawn it bare. None when the profile cannot be built.
pub fn player_bundle(
    data: &GameData,
    rules: &gd_sim::actor::Rules,
    loadout: Loadout,
    level: i32,
) -> Option<(Player, Combatant, Loadout)> {
    let profile = loadout.profile(data, rules, level)?;
    let radius = gd_game_core::combat::get_radius(data.db.get(player_record(loadout.female)), 0.5);
    Some((
        Player,
        Combatant::new(std::sync::Arc::new(profile), radius),
        loadout,
    ))
}

/// [`SpawnData`] from `gdc` (None: a new character of sex `female_default`); None when the profile cannot be built.
pub fn spawn_data(
    data: &GameData,
    rules: &gd_sim::actor::Rules,
    gdc: Option<&Gdc>,
    female_default: bool,
    unlock: impl FnOnce(&mut SkillBook),
) -> Option<SpawnData> {
    // Header byte = `GetGender() == 1` (male), `Character::Load`.
    let female = gdc.map_or(female_default, |g| g.header.sex == 0);
    let level = gdc.map_or(1, |g| g.bio.level as i32);
    let mut skills = skill_book_from_save(&data.db, gdc);
    unlock(&mut skills);
    let loadout = Loadout::from_save(&data.db, &rules.levels, gdc, female, skills);
    let equipment = loadout
        .items
        .iter()
        .map(|i| gd_sim::items::illusion::look_record(i).to_owned())
        .collect();
    Some(SpawnData {
        female,
        equipment,
        bundle: player_bundle(data, rules, loadout, level)?,
    })
}

/// Recomputes [`EffectiveSkills`] whenever the loadout changes (and adds it
/// to a newly spawned player).
pub fn derive_effective_skills(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (Entity, &Loadout, Option<&mut EffectiveSkills>),
        (With<Player>, Changed<Loadout>),
    >,
) {
    for (e, loadout, current) in &mut q {
        let book = loadout.effective_skills(&data.db, &data.skill_trees());
        match current {
            Some(mut c) => {
                if c.0 != book {
                    c.0 = book;
                }
            }
            None => {
                commands.entity(e).insert(EffectiveSkills(book));
            }
        }
    }
}

/// An item toggle whose item comes off (module docs: item toggles,
/// `SkillManager::RemoveItemSkills`): one that was on
/// starts its cooldown (`Skill::StartCooldown`: the skill's own cooldown
/// reduction; an item skill takes no character part) and stops (`StopSkill`:
/// `skillDeactivatedSound`); the skill object goes, so its off / dispelled
/// state does not outlive it (`inactive` / `dispelled` lose the name) and a
/// new item skill starts off, then on at the next update (`AutoActivateNow`).
/// A set's toggle still granted after its piece count changed is removed
/// and granted again the same way (`ItemSet::SkillAugmentInstall` /
/// `SkillAugmentRemove`, [`gd_sim::items::sets::regranted_set_skills`]):
/// stopped with its cooldown started, then on at the next update (the
/// auto toggle does not test the cooldown). Dormant in stock (every set
/// grants its skill only at its full count).
///
/// Every item skill removed is stopped (`RemoveItemSkills` calls the virtual
/// `StopSkill` on each): a summon's stop kills its pets
/// (`Skill_SpawnPet::StopSkill` / `Skill_TargetedSpawnPet::StopSkill`,
/// `crate::pets` docs), sent as a [`crate::pets::StopSummonSkill`].
/// PORT: pets are known by their summon record, not the skill object, so a
/// record also granted by another worn item (or learned) keeps its pets.
#[allow(clippy::type_complexity)]
pub fn unequipped_toggles(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            Option<&Transform>,
            &mut Loadout,
            Option<&mut ItemToggles>,
            Option<&mut gd_game_core::casting::Caster>,
        ),
        (With<Player>, Changed<Loadout>),
    >,
) {
    for (e, tf, mut loadout, current, caster) in &mut q {
        let now = loadout.item_toggles(&data.db);
        let sets = gd_sim::items::sets::worn_sets(&data.db, &loadout.items);
        let mut skills: Vec<String> = gd_sim::skills::item_skill_grants(&data.db, &loadout.items)
            .into_iter()
            .map(|g| g.skill)
            .filter(|s| loadout.skills.level(s) == 0)
            .collect();
        skills.sort_unstable_by(|a, b| {
            a.bytes()
                .map(|c| c.to_ascii_lowercase())
                .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
        });
        skills.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let Some(mut current) = current else {
            commands
                .entity(e)
                .try_insert(ItemToggles(now, sets, skills));
            continue;
        };
        if current.0 == now && current.1 == sets && current.2 == skills {
            continue;
        }
        let regranted = gd_sim::items::sets::regranted_set_skills(
            &current.1,
            &sets,
            &gd_sim::items::sets::set_skills(&data.db, &loadout.items),
        );
        let kept_again = |s: &str| {
            now.iter().any(|n| n.0.eq_ignore_ascii_case(s))
                && regranted.iter().any(|r| r.eq_ignore_ascii_case(s))
        };
        let gone: Vec<(String, u32)> = current
            .0
            .iter()
            .filter(|(s, _)| {
                (!now.iter().any(|n| n.0.eq_ignore_ascii_case(s)) || kept_again(s))
                    && loadout.skills.level(s) == 0
            })
            .cloned()
            .collect();
        let mut caster = caster;
        for (s, level) in &gone {
            let was_on = !loadout.skills.switched_off(&s.to_ascii_lowercase());
            if was_on {
                if let (Some(c), Some(r)) = (caster.as_mut(), data.db.get(s)) {
                    let li = (*level).max(1) as usize - 1;
                    let cd = crate::casting::cooldown_for(r, li, &[], 0.0);
                    c.cooldowns.start(s.clone(), cd, 1);
                }
                if let Some(pak) = crate::combat_sounds::deactivate(&data, s) {
                    commands.write_message(gd_game_core::visuals::PlaySound {
                        pak,
                        at: tf.map(|t| t.translation),
                    });
                }
            }
            debug!("item toggle {s} gone with its item (was on: {was_on})");
        }
        let low: Vec<String> = gone.iter().map(|g| g.0.to_ascii_lowercase()).collect();
        if low
            .iter()
            .any(|s| loadout.skills.inactive.contains(s) || loadout.skills.dispelled.contains(s))
        {
            for s in &low {
                loadout.skills.inactive.remove(s);
                loadout.skills.dispelled.remove(s);
            }
        }
        // A regranted set toggle is a new item skill: off until the next
        // update's auto toggle.
        for (s, _) in gone.iter().filter(|g| kept_again(&g.0)) {
            loadout.skills.inactive.insert(s.to_ascii_lowercase());
        }
        // Every item skill removed (or regranted) stops: a summon's pets die.
        for s in current.2.iter().filter(|s| {
            !skills.iter().any(|n| n.eq_ignore_ascii_case(s))
                || regranted.iter().any(|r| r.eq_ignore_ascii_case(s))
        }) {
            debug!("item skill {s} gone with its item: stopped");
            commands.write_message(crate::pets::StopSummonSkill {
                owner: e,
                skill: s.clone(),
            });
        }
        current.0 = now;
        current.1 = sets;
        current.2 = skills;
    }
}

/// Rebuilds the profile of a player marked [`ProfileDirty`] at its current
/// level; life keeps its fraction of the maximum.
pub fn rebuild_profile(
    mut commands: Commands,
    data: Res<GameData>,
    rules: Res<CombatRules>,
    // Option: the profile keeps its level without progress.
    progress: Option<Res<PlayerProgress>>,
    mut q: Query<
        (
            Entity,
            &Loadout,
            &mut Combatant,
            Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
            Option<&crate::challenge_areas::AreaRules>,
            Option<&gd_game_core::weapon_pool::PoolCharges>,
        ),
        With<ProfileDirty>,
    >,
) {
    for (e, loadout, mut cb, auras, area, charges) in &mut q {
        commands.entity(e).remove::<ProfileDirty>();
        let level = progress
            .as_ref()
            .map_or(cb.profile.level, |p| p.progress.level);
        // In a challenge area: its rules (mutators).
        let rules = area.map_or(&rules.0, |a| &a.0);
        let Some(p) = loadout.profile_with(&data, rules, level, auras, charges) else {
            continue;
        };
        let frac = cb.life / cb.profile.sheet.life.max(1.0);
        cb.profile = std::sync::Arc::new(p);
        cb.life = (frac * cb.profile.sheet.life).max(1.0);
    }
}
