use super::*;

/// The owner's pet bonus records with their level index (see the module docs).
pub fn pet_bonuses(
    data: &crate::data::GameData,
    loadout: &Loadout,
) -> Vec<(gd_formats::arz::Record, usize)> {
    pet_bonuses_with(data, loadout, None)
}

/// [`pet_bonuses`] plus the bonuses of the buffs other sources keep on the
/// owner (`owner_buffs`: a Crucible power-up's buff).
pub fn pet_bonuses_with(
    data: &crate::data::GameData,
    loadout: &Loadout,
    owner_buffs: Option<&crate::skill_buffs::ReceivedBuffs>,
) -> Vec<(gd_formats::arz::Record, usize)> {
    pet_bonus_sources(data, loadout, owner_buffs)
        .into_iter()
        .map(|(_, b, li)| (b, li))
        .collect()
}

/// [`pet_bonuses`] with the skill or buff record each comes from (`None`:
/// an item's, which the engine installs with skill id 0).
pub(super) fn pet_bonus_sources(
    data: &crate::data::GameData,
    loadout: &Loadout,
    owner_buffs: Option<&crate::skill_buffs::ReceivedBuffs>,
) -> Vec<(Option<String>, gd_formats::arz::Record, usize)> {
    let db = &data.db;
    let mut out = Vec::new();
    // Which learned skills have their bonus installed (`Skill::InstallPetBonus`
    // @0x18049f2d0): a `Skill_Passive` on every
    // level change, a toggle (`Skill_BuffSelfToggled::SkillLevelChange` /
    // `InitializeBuff`) while it is on (+220); the skill's modifiers install
    // with it. The level is `GetCurrentLevel` (the learned level with the
    // gear's bonus). Duration buffs: from the running buffs below.
    let trees = data.skill_trees();
    let eff = loadout.effective_skills(db, &trees);
    let base_of = |name: &str| trees.parent(name, false);
    let installs = |name: &str| -> bool {
        let Some(r) = db.get(name) else { return false };
        match r.class.as_str() {
            "Skill_BuffSelfToggled" => loadout.skill_active(db, name),
            "Skill_BuffSelfDuration" => false,
            _ => true,
        }
    };
    for (name, _) in &loadout.skills.skills {
        let level = eff.effective_level(db, name);
        if level == 0 {
            continue;
        }
        let on = match base_of(name) {
            Some(base) => installs(base) && eff.effective_level(db, base) > 0,
            None => installs(name),
        };
        if !on {
            continue;
        }
        if let Some(b) = db.get_owned(name).and_then(|r| {
            r.str("petBonusName")
                .filter(|s| !s.is_empty())
                .and_then(|p| db.get_owned(p))
        }) {
            out.push((Some(name.clone()), b, level as usize - 1));
        }
    }
    // Running buffs: a `Skill_BuffSelfDuration` is a
    // `Skill_BuffSelfToggled` whose `ActivateNow` @0x1804eab10 sends the
    // active state for `skillActiveDuration` and whose `Update` @0x1804eaa70
    // sends the inactive one when it runs out; `ReceiveActiveUpdate`
    // @0x1804ebb50 → `InitializeBuff` @0x1804ec020 installs the pet bonus
    // while on (+220) and removes it (`Skill::RemovePetBonus` @0x18049f860)
    // when off. A buff record on a character (`SkillBuff::Install`
    // @0x1805265f0 / `UnInstall` @0x1805267b0: a secondary's or celestial
    // power's buff, a radius buff on the caster) installs its own
    // `petBonusName` for the character it is on and removes it with the
    // buff. The bonus goes to the owner's list (`GameEngine::SetPetBonus`),
    // which every pet reads live: all its pets while the buff runs, those
    // summoned during it too; a pet's death removes nothing. So: the
    // `Loadout::buffs` entries (their level index from the cast; their
    // learned modifiers are entries too) and the buff records of the
    // switched-on radius toggles, which the engine puts on the caster too
    // (`auras.rs`; PORT: the port keeps those off the player's buff list).
    let buff_class = |r: &gd_formats::arz::Record| {
        r.class.ends_with("BuffSelfDuration") || r.class.starts_with("SkillBuff")
    };
    let mut running: Vec<(String, usize)> = loadout
        .buffs
        .stat_records(db)
        .into_iter()
        .filter(|(r, _)| {
            buff_class(r)
                || (r.class == "Skill_Modifier"
                    && base_of(&r.name)
                        .and_then(|b| db.get(b))
                        .is_some_and(buff_class))
        })
        .map(|(r, li)| (r.name.clone(), li))
        .collect();
    // `SkillBuff::Install` (above): a buff another character keeps on
    // the owner installs its `petBonusName` for the owner too (a Crucible
    // power-up's buff, `story::cast_story_skills`).
    running.extend(
        owner_buffs
            .into_iter()
            .flat_map(|a| &a.list)
            .map(|b| (b.buff.clone(), b.level_index)),
    );
    for (name, l) in &loadout.skills.skills {
        if *l == 0
            || !db
                .get(name)
                .is_some_and(|r| r.class == "Skill_BuffRadiusToggled")
            || !loadout.skill_active(db, name)
        {
            continue;
        }
        let li = loadout.skill_level(db, &eff, name).max(1) as usize - 1;
        if let Some(e) = crate::auras::emission(db, name, li, Vec::new(), true).filter(|e| !e.debuf)
        {
            running.push((e.buff, li));
        }
    }
    for (name, li) in running {
        if let Some(b) = db.get_owned(&name).and_then(|r| {
            r.str("petBonusName")
                .filter(|s| !s.is_empty())
                .and_then(|p| db.get_owned(p))
        }) {
            out.push((Some(name), b, li));
        }
    }
    for it in &loadout.items {
        for rec in [
            &it.base_name,
            &it.prefix_name,
            &it.suffix_name,
            &it.modifier_name,
            &it.relic_name,
            &it.relic_bonus,
            &it.augment_name,
            &it.ascendant_name,
        ] {
            if let Some(b) = db.get_owned(rec).and_then(|r| {
                r.str("petBonusName")
                    .filter(|s| !s.is_empty())
                    .and_then(|p| db.get_owned(p))
            }) {
                out.push((None, b, 0));
            }
        }
    }
    // Item sets (`ItemSet::PetBonusInstall`): the set's
    // `petBonusName` (level 0) with n pieces worn when `petBonusLevel[n - 1]`
    // (clamped to the last) is non-zero, or always when that list is empty.
    let mut worn: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for it in &loadout.items {
        if let Some(set) = db.get(&it.base_name).and_then(|r| {
            r.str("itemSetName")
                .filter(|s| !s.is_empty())
                .map(str::to_ascii_lowercase)
        }) {
            *worn.entry(set).or_default() += 1;
        }
    }
    for (set, n) in worn {
        let Some(sr) = db.get(&set) else { continue };
        let Some(b) = sr
            .str("petBonusName")
            .filter(|s| !s.is_empty())
            .and_then(|p| db.get_owned(p))
        else {
            continue;
        };
        let level = sr
            .f32s_iter("petBonusLevel")
            .nth(n - 1)
            .or_else(|| sr.f32s_iter("petBonusLevel").last());
        if level.is_none_or(|v| v != 0.0) {
            out.push((None, b, 0));
        }
    }
    out
}

/// Whether a pet bonus record reaches a pet with these `petTypeNames`.
pub(super) fn bonus_applies(bonus: &gd_formats::arz::Record, pet_types: &[String]) -> bool {
    gd_sim::skills::pet_bonus_applies(bonus, pet_types)
}

/// The pet bonus character effects a pet wears: (the skill or buff record
/// whose bonus it is, its `charFxPakPetNames` pak), in bonus order (module
/// docs). Derived from [`pet_bonuses`]' sources by [`sync_pet_bonus_fx`],
/// which also starts their [`crate::casting::BuffFx`] effects.
#[derive(Component, Default, Debug, PartialEq)]
pub struct PetBonusFx(pub Vec<(String, String)>);

/// The [`crate::casting::BuffFx::skill`] key of a pet bonus effect: the
/// engine keys it by the source skill's object id, apart from the buffs and
/// toggles on the pet itself.
pub fn pet_bonus_fx_key(source: &str) -> String {
    format!("petbonus:{source}")
}

/// The pet bonus effects a pet with these `petTypeNames` wears (module
/// docs): every skill-installed bonus that reaches it with a
/// `charFxPakPetNames` entry at its level.
pub(super) fn bonus_fx(
    bonuses: &[(Option<String>, gd_formats::arz::Record, usize)],
    pet_types: &[String],
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (src, b, li) in bonuses {
        let Some(src) = src else { continue };
        if !bonus_applies(b, pet_types) {
            continue;
        }
        if let Some(pak) = crate::skill_fx::level_str(b, "charFxPakPetNames", *li)
            && !out.iter().any(|(s, _)| s == src)
        {
            out.push((src.clone(), pak));
        }
    }
    out
}

/// Keeps every pet's [`PetBonusFx`] and their effects in step with its
/// owner's pet bonuses: re-read when the owner's loadout changes and for a
/// new pet (`Player::PostPetSpawn`); a dead pet wears none.
pub(super) fn sync_pet_bonus_fx(
    mut commands: Commands,
    data: Res<GameData>,
    owners: Query<(Entity, Ref<Loadout>)>,
    mut pets: Query<(Entity, &Pet, Option<&mut PetBonusFx>, Has<Dead>)>,
    fx: Query<(Entity, &crate::casting::BuffFx)>,
) {
    for (owner, loadout) in &owners {
        let mut sources = None;
        for (e, pet, have, dead) in pets.iter_mut().filter(|(_, p, ..)| p.owner == owner) {
            if !loadout.is_changed() && have.as_ref().is_some_and(|h| !dead || h.0.is_empty()) {
                continue;
            }
            let want = if dead {
                Vec::new()
            } else {
                let sources =
                    sources.get_or_insert_with(|| pet_bonus_sources(&data, &loadout, None));
                let types: Vec<String> = data
                    .db
                    .get(&pet.record)
                    .map(|r| r.strs_iter("petTypeNames").map(|s| s.to_string()).collect())
                    .unwrap_or_default();
                bonus_fx(sources, &types)
            };
            let old = have.as_ref().map(|h| h.0.clone()).unwrap_or_default();
            if have.is_some() && old == want {
                continue;
            }
            for (f, b) in &fx {
                if b.owner == e
                    && old.iter().any(|(s, p)| {
                        b.skill == pet_bonus_fx_key(s) && !want.contains(&(s.clone(), p.clone()))
                    })
                {
                    commands.entity(f).try_despawn();
                }
            }
            for (src, pak) in want.iter().filter(|w| !old.contains(w)) {
                debug!("{e}: pet bonus effect {pak} ({src})");
                crate::casting::attach_buff_fx(
                    &mut commands,
                    &data,
                    pak,
                    e,
                    &pet_bonus_fx_key(src),
                    false,
                );
            }
            match have {
                Some(mut h) => h.0 = want,
                None => {
                    commands.entity(e).try_insert(PetBonusFx(want));
                }
            }
        }
    }
}
