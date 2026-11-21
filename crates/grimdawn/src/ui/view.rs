//! The HUD's view of the player ([`HudValues`] and [`HudVitals`]), assembled in one place from
//! the gameplay state every frame: life and the sheet from the player's
//! `Combatant` profile, skills from its `EffectiveSkills`, energy, the
//! hotbar and its cooldowns from its `Caster`, level / points / gold from `PlayerProgress`,
//! the buff bar from `gd_game::buff_bar::PlayerIcons` and its `Afflictions`
//! (kept in a [`BuffBar`]), the pets' portrait buff icons, the mode label from
//! `GameMode`, the area name from the map's name sector at the player. Gameplay systems never
//! write UI state; the windows read only this view (or the gameplay
//! components directly), so they cannot drift from what the game uses.
use super::status::{BuffBar, DotIcon, debuff_icon};
use super::{HudValues, HudVitals};
use crate::afflictions::Afflictions;
use crate::casting::Caster;
use crate::combat::{CombatRules, Combatant};
use crate::data::GameData;
use crate::loot::PlayerProgress;
use crate::player::{EffectiveSkills, Player};
use crate::session::LoadedCharacter;
use crate::sheet_dps::{SheetDps, SheetDpsValues};
use bevy::prelude::*;

type PlayerParts<'a> = (
    Entity,
    &'a Combatant,
    Option<&'a EffectiveSkills>,
    Option<&'a Caster>,
    Option<&'a Afflictions>,
    Option<Ref<'a, crate::player::Loadout>>,
    Option<&'a gd_game::weapon_pool::PoolCharges>,
    Option<&'a gd_game::skill_buffs::ReceivedBuffs>,
    Option<&'a gd_game::casting::Shields>,
    Option<&'a gd_game::combo::Combo>,
    Option<Ref<'a, gd_game::weapon_pool::SkillLinks>>,
    Option<&'a gd_game::casting::PotionOverTime>,
);

/// The buff bar's state kept across frames: the bar, the player's status list ([`gd_game::buff_bar::PlayerIcons`],
/// rewritten in place), the duration-damage icons and each listed record's resolved bitmap.
#[derive(Default)]
pub struct BarCache {
    bar: BuffBar,
    icons: gd_game::buff_bar::PlayerIcons,
    listed: Vec<gd_game::buff_bar::Tracked>,
    dots: Vec<DotIcon>,
    /// A record's icon bitmap (`skillUpBitmapName`, else its display record's; `None`: no icon).
    bitmaps: std::collections::HashMap<String, Option<String>>,
}

/// Assigns `new` to `dst` when it differs; true when it did.
fn put<T: PartialEq>(dst: &mut T, new: T) -> bool {
    let differs = *dst != new;
    if differs {
        *dst = new;
    }
    differs
}

/// Fills [`HudValues`] and [`HudVitals`] from the sources they show; fields whose source does
/// not exist yet keep their value.
///
/// Runs every frame, so nothing is rebuilt that did not change: [`HudValues`] is written field by field where a
/// value differs (and marked changed only then), the sheet rows and the hotbar icons are recomputed when their
/// inputs change (see [`SheetCache`]), and strings are compared before they are assigned. [`HudVitals`] (life,
/// energy, cooldowns, the buff bar) is rewritten in place every frame.
#[allow(clippy::too_many_arguments)]
pub fn collect(
    data: Res<GameData>,
    rules: Option<Res<CombatRules>>,
    progress: Res<PlayerProgress>,
    loaded: Option<Res<LoadedCharacter>>,
    travel: (
        Option<Res<crate::riftgates::TravelState>>,
        Res<crate::riftgates::PersonalPortal>,
        Res<crate::world::Levels>,
        Query<&Transform, With<Player>>,
        Option<NonSend<gd_game::story::StoryRuntime>>,
        Res<gd_game::pause::GamePause>,
        Res<super::menufolder::MenuFolder>,
        (
            Res<super::gamemenu::GameMenu>,
            Res<super::options::OptionsWindow>,
        ),
    ),
    dps: Res<SheetDpsValues>,
    (row, bindings): (Res<super::hud::HotbarRow>, Res<super::keys::KeyBindings>),
    // Option: both are created with the player (`gd_game::reclamation`, `gd_game::devotion`).
    // Option: `PlayerFactions` is created with the player too (`gd_game::factions::init_factions`).
    (skill_undo, reclaim, devotion_state, stash, factions): (
        Res<gd_game::points::SkillUndo>,
        Option<Res<gd_game::reclamation::SkillReclamation>>,
        Option<Res<gd_game::devotion::DevotionState>>,
        Option<Res<gd_game::stash::Stash>>,
        Option<Res<gd_game::factions::PlayerFactions>>,
    ),
    devotion_undo: Res<gd_game::devotion::DevotionUndo>,
    player: Query<PlayerParts, With<Player>>,
    constitution: Query<&gd_game::constitution::Constitution, With<Player>>,
    (mut hud, mut vitals): (ResMut<HudValues>, ResMut<HudVitals>),
    choice: Res<crate::session::Choice>,
    (mode, world, inv, combo_rules): (
        Res<gd_game::session::GameMode>,
        Option<Res<crate::world::StreamedMap>>,
        Res<gd_game::inventory::Inventory>,
        Option<Res<gd_game::combo::ComboRules>>,
    ),
    (pets, mut bar, mut fresh, mut strip_seen): (
        Query<
            (
                Entity,
                &gd_game::pets::Pet,
                Option<&gd_game::casting::DamageAuras>,
                Option<&gd_game::skill_buffs::ReceivedBuffs>,
            ),
            Without<gd_game::combat::Dead>,
        >,
        Local<BarCache>,
        gd_game::session::NewSession,
        Local<StripSeen>,
    ),
    (session, pet_cbs, mut cache, hovered, targets, hooks, challenged, areas): (
        Res<gd_game::play_stats::PlayCounts>,
        Query<
            (
                &gd_game::pets::Pet,
                &Combatant,
                Option<&gd_game::skill_buffs::ReceivedBuffs>,
            ),
            Without<gd_game::combat::Dead>,
        >,
        Local<SheetCache>,
        Res<crate::hover::Hovered>,
        Query<(
            Option<Ref<Afflictions>>,
            Option<Ref<gd_game::skill_buffs::ReceivedBuffs>>,
            Option<&gd_game::monsters::Monster>,
        )>,
        Res<crate::hooks::Hooks>,
        Query<&gd_game::challenge_areas::InChallengeArea, With<Player>>,
        Res<gd_game::challenge_areas::ChallengeAreas>,
    ),
) {
    if fresh.begun() {
        *bar = Default::default();
        *cache = Default::default();
        *strip_seen = Default::default();
    }
    let vitals = &mut *vitals;
    let mut changed = false;
    let v = hud.bypass_change_detection();
    changed |= put(&mut v.skill_undo, skill_undo.available());
    changed |= put(
        &mut v.reclaim,
        super::reclamation::ReclaimView::of(reclaim.as_deref(), devotion_state.as_deref()),
    );
    let dv = super::reclamation::DevotionReclaimView::of(reclaim.as_deref(), || {
        stash
            .as_deref()
            .map_or(0, |st| gd_game::devotion::aether_held(&data, &inv, st))
    });
    changed |= put(&mut v.devotion_reclaim, dv);
    changed |= put(&mut v.devotion_undo, devotion_undo.available());
    if let Some(f) = &factions
        && v.factions != f.list
    {
        v.factions.clone_from(&f.list);
        changed = true;
    }
    // A new character's name is the session's choice.
    let name = match &loaded {
        Some(c) => Some(&c.0.header.name),
        None if !choice.name.is_empty() => Some(&choice.name),
        None => None,
    };
    if let Some(n) = name.filter(|n| **n != v.name) {
        v.name.clone_from(n);
        changed = true;
    }
    if let Some(rules) = &rules {
        let p = &progress;
        changed |= put(&mut v.level, p.progress.level as u32);
        changed |= put(
            &mut v.attribute_points,
            p.progress.attribute_points.max(0) as u32,
        );
        changed |= put(&mut v.skill_points, p.progress.skill_points.max(0) as u32);
        changed |= put(&mut v.experience, p.progress.fraction(&rules.0.levels));
        changed |= put(&mut v.experience_points, p.progress.experience);
        changed |= put(
            &mut v.experience_next,
            gd_sim::progress::Progress::threshold(&rules.0.levels, p.progress.level),
        );
        changed |= put(
            &mut v.max_level,
            p.progress.level >= rules.0.levels.max_level,
        );
        changed |= put(&mut v.gold, p.gold);
    }
    if mode.is_changed() || choice.is_changed() || loaded.as_ref().is_some_and(|l| l.is_changed()) {
        // The Crucible titles its difficulties differently (`Mode::label_tags_in`).
        let survival = travel
            .4
            .as_ref()
            .is_some_and(|r| r.story.borrow().game.survival_mode);
        changed |= put(
            &mut v.mode_tags,
            mode.0.label_tags_in(
                gd_game::session::is_hardcore(loaded.as_deref(), &choice),
                survival,
            ),
        );
    }
    let (state, portal, levels, at, story, pause, folder, (menu, options)) = &travel;
    // The orb HUD's menu folder: its buttons hide under the game menu / options window (`menufolder.rs`).
    changed |= put(
        &mut v.menu_folder,
        super::menufolder::FolderView {
            open: folder.open,
            buttons: folder.open && !pause.menu,
        },
    );
    changed |= put(
        &mut v.options_button,
        super::hud::OptionsButton {
            shown: !options.open,
            down: menu.alone() || options.open,
        },
    );
    // The Shattered Realm's progress bar (`UIEndlessHud`): the floor of the player's region.
    let endless = story.as_ref().map_or(0.0, |rt| {
        let s = rt.story.borrow();
        s.player.region.map_or(0.0, |r| s.endless.progress_of(r))
    });
    changed |= put(&mut v.endless_progress, endless);
    // The area name: the name sector under the player, read from the player's
    // own region's level (`Player::UpdateSelf` 0x1803D24A0: sectors are
    // read per region; the Shattered Realm's regions overlap main-world ground,
    // so a lookup by position alone showed "Conclave of the Three" there).
    // Engine.dll `AmbianceManager::Update` @0x1800320c0 (the area name's
    // writer: engine +2880, read by `Engine::GetAreaNameTag`): every update it is
    // the name sector's tag at the ambiance target (whatever its "show" flag),
    // empty where there is no name sector. The target: `ui/cinematic.rs` `area_names`.
    let region = story
        .as_ref()
        .and_then(|rt| rt.story.borrow().player.region);
    if let (Some(world), Ok(tf)) = (&world, at.single()) {
        let tag = match match region {
            Some(r) => levels.sector_entry_in(&world.map, r, tf.translation, 1),
            None => levels.sector_entry(&world.map, tf.translation, 1),
        } {
            Some(gd_formats::map::sector::SectorEntry::Name { tag, .. }) => tag.as_str(),
            _ => "",
        };
        if v.area_tag != tag {
            v.area_tag = tag.to_owned();
            changed = true;
        }
    }
    // The challenge area (`challenge.rs`).
    changed |= super::challenge::ChallengeHud::refresh(
        &mut v.challenge_area,
        &data,
        challenged.single().ok(),
        &areas,
    );
    if let (Some(state), Ok(tf)) = (state, at.single()) {
        // sub_14020D550 (`v175`: the options window or the game menu is up): the active button
        // (`hudActiveTeleportButton`) hides; the plain one stays drawn, disabled for presses without the disabled
        // bitmap (+641 set, +644 clear: `ButtonStatic` draw sub_140123C50 draws the up bitmap), so it neither
        // vanishes nor greys out (`riftmap::teleport_button` refuses the press while the menu is up).
        let unlocked = gd_game::riftgates::portal_unlocked(&data, story.as_deref());
        let teleport = gd_game::riftgates::portal_button(
            tf.translation,
            region,
            Some(&**levels),
            state,
            portal,
            unlocked,
        );
        let teleport = if pause.menu {
            teleport
                .filter(|t| !t.active)
                .map(|t| gd_game::riftgates::PortalButton {
                    disabled: false,
                    ..t
                })
        } else {
            teleport
        };
        changed |= put(&mut v.teleport, teleport);
    }
    changed |= put(&mut v.weapon_swap, inv.alternate_enabled);
    vitals.damage_taken_debuff = 0.0;
    changed |= put(&mut v.hotbar_row, row.0);
    // The hot slots' key labels follow the bindings (a rebind is a layout change).
    if bindings.is_changed() || v.hotbar_labels.is_empty() {
        let tags = data.tags();
        changed |= put(
            &mut v.hotbar_labels,
            super::hud::slot_labels(&bindings, &tags),
        );
    }
    if let Ok((
        me,
        cb,
        skills,
        caster,
        afflictions,
        loadout,
        pool,
        received,
        shield,
        combo,
        links,
        pot,
    )) = player.single()
    {
        let loadout_changed = loadout.as_ref().is_some_and(|l| l.is_changed());
        let loadout = loadout.as_deref();
        let links_changed = links.as_ref().is_some_and(|l| l.is_changed());
        let links = links.as_deref();
        vitals.health = cb.life;
        vitals.health_max = cb.profile.sheet.life;
        // The health-gain bar: current life plus the potion restore pool still to drain
        // (`Character::GetPotentialLife` @0x18006b1b0), over the limit.
        vitals.health_gain = ((cb.life + pot.map_or(0.0, |p| p.life))
            / cb.profile.sheet.life.max(1.0))
        .clamp(0.0, 1.0);
        vitals.constitution_max = cb.profile.sheet.constitution;
        vitals.constitution = constitution.single().map_or(0.0, |c| c.current);
        if let Some(rules) = &rules {
            // The first living pet (`GameEngine::GetLocalPetList` order).
            let pet = pet_cbs
                .iter()
                .filter(|p| p.0.owner == me)
                .min_by(|a, b| a.0.spawned_at.total_cmp(&b.0.spawned_at));
            let foes = session.last_foes(loaded.as_deref().map(|c| &c.0.stats), mode.0.difficulty);
            let red = gd_sim::sheet::resistance_reduction(
                &afflictions
                    .as_ref()
                    .map(|a| a.debuffs())
                    .unwrap_or_default(),
            );
            // The key holds the profiles' `Arc`s, not their addresses: a freed address can be reused
            // between two collects (a level-up rebuild, a pet resummon).
            let sheet_changed = cache.0.as_ref().is_none_or(|(p, pet_p, d, f, r)| {
                !std::sync::Arc::ptr_eq(p, &cb.profile)
                    || match (pet_p, pet) {
                        (Some(a), Some(b)) => !std::sync::Arc::ptr_eq(a, &b.1.profile),
                        (None, None) => false,
                        _ => true,
                    }
                    || *d != dps.0
                    || *f != foes
                    || *r != red
            });
            if sheet_changed {
                let pet = pet.map(|p| (p.1.profile.clone(), p.0.buff_stats(&data.db, p.2)));
                (cache.1, cache.2) = sheet(
                    cb,
                    &rules.0,
                    &dps.0,
                    &data,
                    loadout,
                    pet.as_ref(),
                    &red,
                    &foes,
                );
                cache.2.insert("DPSNumber".into(), dps.0.roll_values());
                cache.3 = gd_sim::sheet::armor_breakdown(&cb.profile, &rules.0);
                cache.0 = Some((
                    cb.profile.clone(),
                    pet.map(|p| p.0.clone()),
                    dps.0.clone(),
                    foes,
                    red,
                ));
                v.rollovers.clone_from(&cache.2);
                v.armor.clone_from(&cache.3);
                changed = true;
            }
            // The rows the sheet and the play stats add are rebuilt when one of their inputs changed.
            // `session.is_changed()` is true every frame (`play_stats::count_play_time` ticks `play_ms`), so
            // the play stats are keyed on the values the rows read: the minute of play time, the counts
            // and the rounded greatest damage. The greatest monster and the item levels come from the
            // loaded stats / the loadout, whose change detection covers them.
            let ls = loaded.as_deref().map(|c| &c.0.stats);
            let stat_key = (
                (ls.map_or(0, |p| p.play_time) + session.play_seconds) / 60,
                ls.map_or(session.kills, |p| p.kills + session.kills),
                ls.map_or(0, |p| p.champion_kills + session.champion_kills),
                ls.map_or(session.deaths, |p| p.deaths + session.deaths),
                (ls.map_or(0.0, |p| p.greatest_damage_inflicted)
                    .max(session.greatest_damage_inflicted)
                    .max(0.0)
                    + 0.5)
                    .floor() as i64,
                (ls.map_or(0.0, |p| p.greatest_damage_received)
                    .max(session.greatest_damage_received)
                    .max(0.0)
                    + 0.5)
                    .floor() as i64,
            );
            if sheet_changed
                || cache.5.is_empty()
                || cache.6 != stat_key
                || loadout_changed
                || mode.is_changed()
                || loaded.as_ref().is_some_and(|l| l.is_changed())
            {
                let mut stats = cache.1.clone();
                stats.extend(stat_rows(
                    &data,
                    loaded.as_deref(),
                    &session,
                    Some(&*mode),
                    loadout,
                ));
                cache.5 = stats;
                cache.6 = stat_key;
                if cache.5 != v.stats {
                    debug!(
                        "sheet {:?}",
                        cache
                            .5
                            .iter()
                            .filter(|(k, _)| k.ends_with("Number") && !k.contains("Resist"))
                            .take(40)
                            .collect::<Vec<_>>()
                    );
                    publish_rows(&mut v.stats, &cache.5);
                    changed = true;
                }
            }
            if v.skill_breakdown != dps.0.breakdown {
                v.skill_breakdown.clone_from(&dps.0.breakdown);
                changed = true;
            }
        }
        let mut skills_changed = false;
        if let Some(s) = skills
            && v.skills != s.0
        {
            v.skills.clone_from(&s.0);
            skills_changed = true;
            changed = true;
        }
        if let Some(c) = caster {
            // The hotbar icons follow the slots and the learned skills.
            if skills_changed || cache.4 != c.slots || v.hotbar.len() != c.slots.len() {
                v.hotbar = c
                    .slots
                    .iter()
                    .map(|s| {
                        s.as_deref()
                            .and_then(|s| super::hotbar_pick::option_icon(&data, s, &v.skills))
                    })
                    .collect();
                cache.4 = c.slots.to_vec();
                changed = true;
            }
            let max = cb.profile.sheet.energy;
            vitals.energy = c.energy.min(max);
            vitals.energy_max = max;
            vitals.energy_reserved = cb.profile.sheet.energy_reserve;
            super::cooldown::slots(c, v.hotbar_row, &v.hotbar_labels, &mut vitals.cooldowns);
            hot_slot_states(
                &data,
                &v.skills,
                caster,
                loadout,
                links,
                pool,
                rules.as_deref(),
                cb,
                v.hotbar_row,
                // The other inputs of `hot_statics`, which update in their own systems and frames.
                skills_changed
                    || loadout_changed
                    || links_changed
                    || rules.as_ref().is_some_and(|r| r.is_changed()),
                &mut cache.7,
                &mut vitals.hot_slots,
            );
        }
        if let (Some(c), Some(l)) = (caster, loadout) {
            let summons: Vec<&str> = pets
                .iter()
                .filter(|p| p.1.owner == me)
                .map(|p| p.1.summon.as_str())
                .collect();
            let st = gd_game::buff_bar::PlayerState {
                loadout: l,
                caster: c,
                links,
                pool,
                pets: &summons,
                received,
                shield,
                combo: combo.zip(combo_rules.as_deref().map(|r| &r.0)),
            };
            let bar = &mut *bar;
            bar.icons
                .list(&data.db, &st, loadout_changed, &mut bar.listed);
            // The icon is the trackable icon (a `.tex`) or the record's up
            // bitmap (`SkillProfile::GetUpBitmapName`); none: no icon
            // (sub_14024F350). PORT: else its display record's. Resolved once per record.
            let bitmaps = &mut bar.bitmaps;
            bar.listed.retain_mut(|t| {
                if t.icon.ends_with(".tex") {
                    return true;
                }
                if !bitmaps.contains_key(&t.icon) {
                    let up =
                        |r: &gd_formats::arz::Record| r.str("skillUpBitmapName").map(str::to_owned);
                    let own = data.db.get(&t.icon).and_then(up);
                    let b = own
                        .or_else(|| {
                            gd_sim::skills::display_record(&data.db, &t.icon).and_then(|r| up(&r))
                        })
                        .filter(|b| !b.is_empty());
                    bitmaps.insert(t.icon.clone(), b);
                }
                match &bitmaps[&t.icon] {
                    Some(b) => {
                        t.icon.clone_from(b);
                        true
                    }
                    None => false,
                }
            });
            dot_icons(afflictions, &mut bar.dots);
            vitals.damage_taken_debuff = afflictions.map_or(0.0, Afflictions::damage_multiplier);
            bar.bar.update(
                &bar.listed,
                &bar.dots,
                |s| c.cooldowns.get(s).copied().unwrap_or(0.0),
                &mut vitals.status,
            );
        }
        // The pets' portrait icons, compared before they are copied.
        let mut n = 0;
        for (e, pet, da, ab) in pets.iter().filter(|p| p.1.owner == me) {
            let icons = gd_game::buff_bar::pet_icons(&data.db, da, Some(pet), ab);
            match v.pet_buffs.get_mut(n) {
                Some((pe, list)) if *pe == e && list.iter().eq(icons.iter().copied()) => {}
                Some(slot) => {
                    *slot = (e, icons.iter().map(|s| (*s).to_owned()).collect());
                    changed = true;
                }
                None => {
                    v.pet_buffs
                        .push((e, icons.iter().map(|s| (*s).to_owned()).collect()));
                    changed = true;
                }
            }
            n += 1;
        }
        if v.pet_buffs.len() != n {
            v.pet_buffs.truncate(n);
            changed = true;
        }
    }
    // The hovered character's debuff strip (`monsterBarsDebuffs`, `hover.rs`).
    let hovered_parts = hovered
        .entity
        .filter(|_| hovered.kind == crate::hover::Kind::Character)
        .and_then(|e| targets.get(e).ok());
    let target = hovered_parts.and(hovered.entity);
    strip_seen.retarget(target);
    let strip = match hovered_parts {
        Some((a, r, m)) => {
            let src = (
                r.as_ref().map(|r| r.last_changed()),
                r.as_ref().map_or(0, |r| r.list.len()),
                a.as_ref().map(|a| a.last_changed()),
            );
            if strip_seen.src != src {
                strip_seen.strip = target_strip(&data.db, m, r.as_deref(), a.as_deref());
                strip_seen.src = src;
            } else if let Some(r) = &r {
                // The countdowns run down without a change tick: refresh their text.
                for (k, d) in &mut strip_seen.strip {
                    if let Some(rec) = k.strip_prefix("s:")
                        && let Some(b) = r.list.iter().find(|b| b.buff.eq_ignore_ascii_case(rec))
                    {
                        d.text = countdown_text(b.left, b.total);
                    }
                }
            }
            strip_seen.merge()
        }
        None => Vec::new(),
    };
    if v.target_debuffs != strip {
        v.target_debuffs = strip;
        changed = true;
    }
    // `GD_SUNDERED` (hooks.rs): overrides the entry's value (a shot of the overlay without an enemy that applies it).
    if let Some(h) = hooks.ui.sundered {
        vitals.damage_taken_debuff = h;
    }
    if changed {
        hud.set_changed();
    }
}

/// One shown hot slot's inputs that change only with the profile (a skill, gear, weapon or form change rebuilds
/// it, minting a new `Arc`): computing them walks the skill trees for learned modifiers (`energy_cost`) and the
/// worn items for granted skills, too costly for every slot every frame.
pub struct HotStatic {
    field: &'static str,
    skill: String,
    /// Lowercase: the switched-off set's and the pool charges' key.
    key: String,
    is_default: bool,
    learned: bool,
    /// A learned or granted toggle (`Loadout::skill_active`'s static half).
    toggle: bool,
    class: String,
    cost: f32,
    weapons_ok: bool,
    charged: bool,
}

/// [`hot_slot_states`]' cache: the profile, hotbar row and slots its [`HotStatic`]s were built from (the other
/// inputs invalidate it when they change).
#[derive(Default)]
pub struct HotCache(
    Option<(
        std::sync::Arc<gd_sim::actor::Profile>,
        usize,
        Vec<Option<String>>,
    )>,
    Vec<HotStatic>,
);

/// The shown hot slots' state overlays ([`super::hud::HotSlotState`],
/// `HotSlotOptionSkill::GetStatus` @0x18031c140 / sub_1401E6C90): the active-toggle / low-energy blue
/// highlight, the wrong-weapon red highlight, the unlearned dark rect, and the charged pool skills'
/// charge count. The usability is the hot-slot tooltip's (`gd_sim::skill_tooltip::hot_state`). Per frame only
/// the toggles, energy, cooldowns and charges are read; the rest comes from `cache` ([`HotStatic`]), rebuilt
/// when its key moved or `inputs_changed` (the book, loadout, links or rules changed this frame). Written into
/// `out` in place.
#[allow(clippy::too_many_arguments)]
fn hot_slot_states(
    data: &GameData,
    book: &gd_sim::skills::SkillBook,
    caster: Option<&Caster>,
    loadout: Option<&crate::player::Loadout>,
    links: Option<&gd_game::weapon_pool::SkillLinks>,
    pool: Option<&gd_game::weapon_pool::PoolCharges>,
    rules: Option<&CombatRules>,
    cb: &Combatant,
    row: usize,
    inputs_changed: bool,
    cache: &mut HotCache,
    out: &mut Vec<super::hud::HotSlotState>,
) {
    use super::hud::{HotSlotState, SlotLook};
    use gd_sim::skill_tooltip::{HotState, hot_state};
    out.clear();
    let Some(c) = caster else {
        return;
    };
    let stale = inputs_changed
        || match &cache.0 {
            Some((p, r, s)) => {
                !std::sync::Arc::ptr_eq(p, &cb.profile) || *r != row || *s != c.slots
            }
            None => true,
        };
    if stale {
        cache.1 = hot_statics(data, book, c, loadout, links, rules, cb, row);
        cache.0 = Some((cb.profile.clone(), row, c.slots.to_vec()));
    }
    out.extend(cache.1.iter().map(|s| {
        let active = loadout.is_some_and(|l| {
            l.buffs.running(&s.skill) || (s.toggle && !l.skills.switched_off(&s.key))
        });
        let ignores = gd_sim::skill_tooltip::ignores_energy(&s.class, active);
        let energy_ok = s.is_default || ignores || c.energy >= s.cost;
        let cooling = c.cooldowns.get(s.skill.as_str()).is_some_and(|l| *l > 0.0);
        let state = hot_state(energy_ok, cooling, s.weapons_ok);
        let look = if !s.learned {
            SlotLook::Dark
        } else if active {
            SlotLook::Blue
        } else {
            match state {
                HotState::LowMana => SlotLook::Blue,
                HotState::Configuration => SlotLook::Red,
                _ => SlotLook::None,
            }
        };
        let usable = s.learned && (active || state == HotState::Ready);
        // Charged pool skills show their charge count (Cadence's finale shows none, like the buff bar).
        let charges = s
            .charged
            .then(|| pool.and_then(|p| p.0.get(&s.key)).map(|ch| ch.count))
            .flatten();
        HotSlotState {
            field: s.field,
            look,
            usable,
            charges,
        }
    }));
}

/// The shown hot slots' [`HotStatic`]s: evade, the two potions, the row's slots, then the two mouse slots.
#[allow(clippy::too_many_arguments)]
fn hot_statics(
    data: &GameData,
    book: &gd_sim::skills::SkillBook,
    c: &Caster,
    loadout: Option<&crate::player::Loadout>,
    links: Option<&gd_game::weapon_pool::SkillLinks>,
    rules: Option<&CombatRules>,
    cb: &Combatant,
    row: usize,
) -> Vec<HotStatic> {
    use super::hud::HOT_SLOTS;
    use crate::casting::{ENERGY_POTION, HEALTH_POTION, SLOT_LEFT, SLOT_RIGHT, SLOT_ROW2};
    use gd_sim::skill_tooltip::SkillTipCtx;
    let tags = data.tags();
    let ctx = SkillTipCtx {
        db: &data.db,
        trees: &data.skill_trees(),
        tags: &tags,
        book,
        player: rules.map(|r| (&*r.0, &*cb.profile)),
        pet_bonuses: &[],
    };
    let slot = |n: usize| c.slots.get(n).and_then(|s| s.as_deref());
    let shown = [
        ("hudEvadeButton", Some(crate::evade::EVADE)),
        ("hudPriPotionButton", Some(HEALTH_POTION)),
        ("hudSecPotionButton", Some(ENERGY_POTION)),
    ]
    .into_iter()
    .chain(
        HOT_SLOTS
            .iter()
            .enumerate()
            .map(|(i, f)| (*f, slot(i + row * SLOT_ROW2))),
    )
    .chain([
        ("hudPriSkillButton", slot(SLOT_LEFT)),
        ("hudSecSkillButton", slot(SLOT_RIGHT)),
    ]);
    let mut out = Vec::new();
    for (field, skill) in shown {
        let Some(skill) = skill else { continue };
        let is_default =
            skill == crate::evade::EVADE || skill == HEALTH_POTION || skill == ENERGY_POTION;
        let level = book.level(skill);
        let gear = loadout.map_or(0, |l| {
            gd_sim::skills::granted_level(
                &gd_sim::skills::item_skill_grants(&data.db, &l.items),
                skill,
            )
        });
        // An unlearned slotted skill that worn gear or a form grants for a while (skill+761).
        let granted = !is_default
            && level == 0
            && loadout.is_some_and(|l| {
                let form = l
                    .granted
                    .iter()
                    .filter(|(g, _)| g.eq_ignore_ascii_case(skill))
                    .map(|(_, lv)| *lv)
                    .max()
                    .unwrap_or(0);
                gear.max(form) > 0
            });
        let learned = is_default || level > 0 || granted;
        let rec = data.db.get(skill);
        let class = rec.as_ref().map(|r| r.class.clone()).unwrap_or_default();
        let key = skill.to_ascii_lowercase();
        out.push(HotStatic {
            field,
            toggle: loadout.is_some_and(|l| l.skills.level(skill) > 0 || gear > 0)
                && gd_sim::skills::is_toggle(&class),
            cost: if is_default {
                0.0
            } else {
                gd_sim::skill_tooltip::energy_cost(&ctx, skill, granted)
            },
            weapons_ok: match (links, rec.as_ref()) {
                (Some(l), Some(rec)) => l.weapons_allow(&data.db, rec, skill),
                _ => true,
            },
            charged: class.starts_with("Skill_WeaponPool_Charged")
                && class != "Skill_WeaponPool_ChargedFinale",
            skill: skill.to_owned(),
            key,
            is_default,
            learned,
            class,
        });
    }
    out
}

/// A timed skill debuff's icon text (sub_14024F110 type 0): `ms / 1000 + 1` while under 3000 ms are
/// left and the total is known.
fn countdown_text(left: f32, total: f32) -> Option<String> {
    let ms = (left * 1000.0) as i32;
    (ms < 3000 && total > 0.0).then(|| (ms / 1000 + 1).to_string())
}

/// What the hovered character's strip already shows ([`StripSeen::merge`]).
#[derive(Default)]
pub struct StripSeen {
    target: Option<Entity>,
    keys: Vec<String>,
    /// The last [`target_strip`] output and the inputs it was built from: the
    /// received buffs' change tick and count (their timers tick without a
    /// change tick, `gd_game::skill_buffs::run_buffs`; an expiry only shrinks
    /// the list) and the afflictions' change tick.
    strip: Vec<(String, super::status::TargetDebuff)>,
    src: (
        Option<bevy::ecs::change_detection::Tick>,
        usize,
        Option<bevy::ecs::change_detection::Tick>,
    ),
}

impl StripSeen {
    /// sub_14024F350 / sub_14024F890: the widget keeps its icons; each update refreshes an icon in place
    /// (same skill / same duration-damage type), appends the new ones at the end in the update's order
    /// ([`target_strip`]) and drops the ones gone (state 2). So an icon keeps its place for as long as it shows.
    /// A skill icon is keyed by its buff record (the engine's by the buff's object id: a buff made anew moves to
    /// the end there; PORT). For the target change see [`StripSeen::retarget`].
    /// `Grim Dawn.exe` sub_140021000 / sub_14002BC00 (the hover writes the widget's target, +0x6D0 of
    /// the monster bar = widget +48, and nothing else) and sub_14024C7D0 (an icon not refreshed by an update goes
    /// to state 2: `SkillBuff`'s vfunc +1464 is the stock `return false`; the next update drops it): the widget
    /// keeps its icons across a target change. The old target's skill icons are keyed by its buff objects, which
    /// the new target never refreshes, so they go; a duration-damage icon is keyed by its type (sub_14024F890),
    /// so one the new target also has keeps its place. PORT: the dropped icons show one update less.
    fn retarget(&mut self, target: Option<Entity>) {
        if self.target != target {
            self.target = target;
            self.keys.retain(|k| k.starts_with("d:"));
            // The new target's ticks may equal the old one's: its strip is computed anew.
            self.src = Default::default();
            self.strip.clear();
        }
    }

    fn merge(&mut self) -> Vec<super::status::TargetDebuff> {
        let mut out: Vec<(String, super::status::TargetDebuff)> =
            Vec::with_capacity(self.strip.len());
        let mut fresh: Vec<Option<&(String, super::status::TargetDebuff)>> =
            self.strip.iter().map(Some).collect();
        for k in &self.keys {
            if let Some(e) = fresh.iter_mut().find(|e| e.is_some_and(|e| e.0 == *k)) {
                out.extend(e.take().cloned());
            }
        }
        out.extend(fresh.into_iter().flatten().cloned());
        self.keys = out.iter().map(|e| e.0.clone()).collect();
        out.into_iter().map(|e| e.1).collect()
    }
}

/// The debuff strip of the target info widget in one update's order (sub_14024E790): the target's active
/// skills (its own: no `SkillBuff_Debuf` among a monster's), then its UI temp skill list, then the item skill
/// list, then its duration-damage types (normal DoTs
/// in insertion order, then the fixed-damage ones, [`Afflictions::status`]; one icon per type, no text).
/// The temp skill list holds the buffs received (`SkillManager::CreateRemoveSkillBuff` @0x18053a990:
/// after every buff made or removed the list +320 is rebuilt from the buff map's list,
/// [`gd_sim::hash_order::msvc_unordered_order`] of the
/// buff records in the order they were made); the strip takes its `SkillBuff_Debuf`s without `hideFromUI`
/// (`SkillBuff::IsTempTrackable`, `SkillBuff::LoadResources` @0x180526010). A debuff's icon is its own record's
/// `skillUpBitmapName` (the first of a list; `Skill::GetTrackableIcon` is empty for buffs, then
/// `SkillProfile::GetUpBitmapName(1)` of the buff's own profile): a debuff record without one shows nothing (the
/// stock pet auras of the Arcane Seal and the Elemental Seeker, item bleed vulnerabilities). A timed one under 3 s
/// left shows `ms / 1000 + 1` in red. Keys: `a:` + an own skill, `s:` + the buff record, `d:` + the DoT icon
/// ([`StripSeen`]).
/// The active list (Game.dll `SkillManager::GetActiveSkillList` @0x180531780: list +552 first) gets every
/// own skill set to a level above 0 (`Skill::SetSkillLevel` → `AddToActiveList`; received buffs and sub-skills are
/// flagged +760 by `CreateSkillBuff` / `AddSubSkills` and stay off it), in load order: for a monster its
/// `skillName1..` with a level ([`gd_sim::actor::monster_skill_levels`]); the strip shows those that are
/// `SkillBuff_Debuf`s, untimed (stock: Ordarion `cult_h01`'s `ordarion_chaosdrain_buff`). A monster's temp list is
/// its [`ReceivedBuffs`](gd_game::skill_buffs::ReceivedBuffs) like the player's. The item skill list adds nothing in
/// stock data (no `itemSkillName` / augment skill is a `SkillBuff_Debuf`; their debuffs land on the hit target's
/// temp list). PORT: the order the buffs were made is the received-buff list (newest first) walked backwards; buffs
/// removed earlier do not count.
pub fn target_strip(
    db: &gd_formats::vfs::Database,
    monster: Option<&gd_game::monsters::Monster>,
    received: Option<&gd_game::skill_buffs::ReceivedBuffs>,
    afflictions: Option<&Afflictions>,
) -> Vec<(String, super::status::TargetDebuff)> {
    let icon_of = |path: &str| {
        db.get(path)
            .filter(|r| gd_game::skill_buffs::is_debuff(r) && !r.bool("hideFromUI"))
            .and_then(|r| {
                r.str("skillUpBitmapName")
                    .and_then(|i| i.split(';').next())
                    .filter(|i| !i.is_empty())
                    .map(str::to_owned)
            })
    };
    let mut out: Vec<(String, super::status::TargetDebuff)> = monster
        .and_then(|m| {
            db.get(&m.record)
                .map(|r| gd_sim::actor::monster_skill_levels(r, m.level))
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(skill, _)| {
            let icon = icon_of(&skill)?;
            Some((
                format!("a:{}", skill.to_ascii_lowercase()),
                super::status::TargetDebuff { icon, text: None },
            ))
        })
        .collect();
    use super::status::TargetDebuff;
    let made: Vec<&gd_game::skill_buffs::ReceivedBuff> = received
        .map(|r| r.list.iter().rev().collect())
        .unwrap_or_default();
    let names: Vec<&str> = made.iter().map(|b| b.buff.as_str()).collect();
    for i in gd_sim::hash_order::msvc_unordered_order(&names) {
        let b = made[i];
        let Some(icon) = icon_of(&b.buff) else {
            continue;
        };
        out.push((
            format!("s:{}", b.buff.to_ascii_lowercase()),
            TargetDebuff {
                icon,
                text: countdown_text(b.left, b.total),
            },
        ));
    }
    let mut dots = Vec::new();
    dot_icons(afflictions, &mut dots);
    for d in dots {
        out.push((
            format!("d:{}", d.icon),
            TargetDebuff {
                icon: d.icon.to_owned(),
                text: None,
            },
        ));
    }
    out
}

/// The sheet rows that only change with the profile, the first pet or the
/// DPS values (they run the damage accumulator: not every frame), and the
/// hot slots / merged rows derived from them.
#[derive(Default)]
pub struct SheetCache(
    /// The sheet's inputs: the player's and the pet's profile (held, compared with `Arc::ptr_eq`), the
    /// DPS values, the last foes and the resistance reduction.
    Option<(
        std::sync::Arc<gd_sim::actor::Profile>,
        Option<std::sync::Arc<gd_sim::actor::Profile>>,
        SheetDps,
        gd_sim::sheet::LastFoes,
        gd_sim::combat::ReductionInfo,
    )>,
    std::collections::BTreeMap<String, String>,
    std::collections::BTreeMap<String, gd_sim::sheet::RollValues>,
    gd_sim::sheet::ArmorBreakdown,
    /// The caster's hot slots the hotbar icons were built from.
    Vec<Option<String>>,
    /// Sheet rows + play stat rows (what `HudValues::stats` shows).
    std::collections::BTreeMap<String, String>,
    /// The play-stat values the stat rows were built from (the minute of play time, kills, champion
    /// kills, deaths, the rounded greatest damage dealt / taken).
    (u32, u32, u32, u32, i64, i64),
    /// The hot slots' profile-static state ([`hot_slot_states`]).
    HotCache,
);

/// Publishes `rows` into `dst` field-wise: only the rows that differ are written, the rest keep their
/// `String`s.
fn publish_rows(
    dst: &mut std::collections::BTreeMap<String, String>,
    rows: &std::collections::BTreeMap<String, String>,
) {
    dst.retain(|k, _| rows.contains_key(k));
    for (k, v) in rows {
        match dst.get_mut(k) {
            Some(old) => {
                if old != v {
                    old.clone_from(v);
                }
            }
            None => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// The page III Stats rows: the save's play stats plus this session's
/// (`PlayCounts::apply`), the greatest monster of the current difficulty
/// (`PlayCounts::greatest`), the worn items' levels.
fn stat_rows(
    data: &GameData,
    loaded: Option<&LoadedCharacter>,
    session: &gd_game::play_stats::PlayCounts,
    mode: Option<&gd_game::session::GameMode>,
    loadout: Option<&crate::player::Loadout>,
) -> std::collections::BTreeMap<String, String> {
    let mut ps = loaded.map(|c| c.0.stats.clone());
    if let Some(ps) = &mut ps {
        session.apply(ps);
    }
    let difficulty = mode.map_or(0, |m| m.0.difficulty);
    let st = gd_sim::sheet::SheetStats {
        play_seconds: ps.as_ref().map_or(0, |p| p.play_time),
        kills: ps.as_ref().map_or(session.kills, |p| p.kills),
        champion_kills: ps.as_ref().map_or(0, |p| p.champion_kills),
        greatest_damage_dealt: ps.as_ref().map_or(0.0, |p| p.greatest_damage_inflicted),
        greatest_damage_taken: ps.as_ref().map_or(0.0, |p| p.greatest_damage_received),
        deaths: ps.as_ref().map_or(session.deaths, |p| p.deaths),
        greatest_monster: session
            .greatest(loaded.map(|c| &c.0.stats), difficulty)
            .name,
        item_levels: loadout
            .map(|l| {
                l.items
                    .iter()
                    .filter_map(|i| data.db.get(&i.base_name).and_then(|r| r.f32("itemLevel")))
                    .collect()
            })
            .unwrap_or_default(),
    };
    gd_sim::sheet::stat_rows(&st)
}

/// The character window's numbers (the profile's sheet, the DPS rows, the
/// skill bonus rows and the pet bonus rows) and the rollovers.
#[allow(clippy::too_many_arguments)]
fn sheet(
    cb: &Combatant,
    rules: &gd_sim::actor::Rules,
    dps: &SheetDps,
    data: &GameData,
    loadout: Option<&crate::player::Loadout>,
    pet: Option<&(
        std::sync::Arc<gd_sim::actor::Profile>,
        Vec<(gd_formats::arz::Record, usize)>,
    )>,
    reduction: &gd_sim::combat::ReductionInfo,
    foes: &gd_sim::sheet::LastFoes,
) -> (
    std::collections::BTreeMap<String, String>,
    std::collections::BTreeMap<String, gd_sim::sheet::RollValues>,
) {
    let (mut stats, rollovers) = gd_sim::sheet::numbers_and_rollovers(
        &cb.profile,
        rules,
        dps.number(),
        dps.attacks_per_second,
        reduction,
        foes,
        dps.weapon_hands,
    );
    if let Some(l) = loadout {
        let (all, masteries) =
            gd_sim::skills::sheet_augments(&data.db, &data.skill_trees(), &l.items);
        stats.extend(gd_sim::sheet::augment_rows(all, masteries));
        stats.extend(gd_sim::sheet::pet_rows(
            &gd_game::pets::pet_bonuses(data, l),
            pet.map_or(&[], |p| &p.1),
            pet.map(|p| &*p.0),
        ));
    }
    // The sheet's page II names the two mouse skills (their display tags).
    for (n, sk) in [&dps.left, &dps.right].into_iter().enumerate() {
        if let Some((tag, _)) = sk {
            stats.insert(format!("list.SkillTag{n}"), tag.clone());
            stats.insert(format!("list.SkillDmg{n}"), SheetDps::hit_text(dps.hits[n]));
        }
    }
    stats.insert("list.AvgDmg".into(), SheetDps::hit_text(dps.weapon_hit));
    (stats, rollovers)
}

/// The duration-damage / debuff / crowd-control types with seconds left,
/// one icon per bitmap, written into `v` (none without afflictions).
fn dot_icons(a: Option<&Afflictions>, v: &mut Vec<DotIcon>) {
    v.clear();
    let Some(a) = a else { return };
    for (ty, secs, magnitude) in a.status_detail() {
        let Some(icon) = debuff_icon(ty) else {
            continue;
        };
        match v.iter_mut().find(|x| x.icon == icon) {
            Some(x) => x.left = x.left.max(secs),
            None => v.push(DotIcon {
                icon,
                left: secs,
                ty,
                magnitude,
            }),
        }
    }
}
