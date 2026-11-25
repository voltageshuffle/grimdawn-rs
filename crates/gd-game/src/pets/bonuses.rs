use super::*;

/// The owner's pet bonus records with their level index.
pub fn pet_bonuses(
    data: &gd_game_core::data::GameData,
    loadout: &Loadout,
) -> Vec<(gd_formats::arz::Record, usize)> {
    pet_bonuses_with(data, loadout, None)
}

/// `owner_buffs`: buffs other sources keep on the owner (a Crucible power-up's).
pub fn pet_bonuses_with(
    data: &gd_game_core::data::GameData,
    loadout: &Loadout,
    owner_buffs: Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
) -> Vec<(gd_formats::arz::Record, usize)> {
    pet_bonus_sources(data, loadout, owner_buffs)
        .into_iter()
        .map(|(_, b, li)| (b, li))
        .collect()
}

/// With each bonus's skill or buff record (`None`: an item's, installed with skill id 0).
pub(super) fn pet_bonus_sources(
    data: &gd_game_core::data::GameData,
    loadout: &Loadout,
    owner_buffs: Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
) -> Vec<(Option<String>, gd_formats::arz::Record, usize)> {
    let db = &data.db;
    let mut out = Vec::new();
    // Learned skills with their bonus installed (`Skill::InstallPetBonus`): a `Skill_Passive` always, a
    // toggle while on (`Skill.active`), modifiers with their skill; at `GetCurrentLevel`. Duration buffs:
    // from the running buffs below.
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
    // Running buffs: a `Skill_BuffSelfDuration` (`InitializeBuff`) or a buff record on a character
    // (`SkillBuff::Install` / `UnInstall`) installs its `petBonusName` while it runs, into the owner's
    // list (`GameEngine::SetPetBonus`) which every pet reads live; a pet's death removes nothing. Switched-on
    // radius toggles put their buff on the caster too (PORT: kept off the player's buff list).
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
    // A buff another character keeps on the owner installs its `petBonusName` for the owner too.
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
            running.push((e.buff.to_string(), li));
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
    // `ItemSet::PetBonusInstall`: the set's `petBonusName` (level 0) with n pieces worn when
    // `petBonusLevel[n - 1]` (clamped to the last) is non-zero, or always when that list is empty.
    let mut worn: gd_formats::collections::HashMap<String, usize> =
        gd_formats::collections::HashMap::default();
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

pub(super) fn bonus_applies(bonus: &gd_formats::arz::Record, pet_types: &[String]) -> bool {
    gd_sim::skills::pet_bonus_applies(bonus, pet_types)
}

/// The pet bonus character effects a pet wears: (source skill or buff record, its `charFxPakPetNames`
/// pak), in bonus order. Kept by [`sync_pet_bonus_fx`].
#[derive(Component, Default, Debug, PartialEq)]
pub struct PetBonusFx(pub Vec<(String, String)>);

/// The engine keys a pet bonus effect by the source skill's object id, apart from the pet's own buffs.
pub fn pet_bonus_fx_key(source: &str) -> String {
    format!("petbonus:{source}")
}

/// Every skill-installed bonus that reaches the pet with a `charFxPakPetNames` entry at its level.
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
        if let Some(pak) = gd_game_core::skill_fx::level_str(b, "charFxPakPetNames", *li)
            && !out.iter().any(|(s, _)| s == src)
        {
            out.push((src.clone(), pak));
        }
    }
    out
}

/// Re-read when the owner's loadout changes and for a new pet (`Player::PostPetSpawn`); a dead pet wears
/// none.
pub(super) fn sync_pet_bonus_fx(
    mut commands: Commands,
    data: Res<GameData>,
    owners: Query<(Entity, Ref<Loadout>)>,
    mut pets: Query<(Entity, &Pet, Option<&mut PetBonusFx>, Has<Dead>)>,
    fx: Query<(Entity, &gd_game_core::visuals::BuffFx)>,
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
