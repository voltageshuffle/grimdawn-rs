//! The session lifecycle systems: [`begin`] starts a session, [`end_session_state`]
//! tears it down, `load_save` (dev builds) starts one from `GD_SAVE`, [`set_mod_data`] mounts the
//! session's mod.
use bevy::prelude::*;
use gd_formats::save::gdc::Gdc;
use gd_game_core::session::{Choice, GameMode, LoadedCharacter, SessionReady, last_difficulty};
#[cfg(feature = "dev")]
use gd_game_core::session::{CustomMap, SessionEpoch, arena_map, mode_for};
use std::sync::Arc;

/// The session state the rules read beyond the database.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Session<'w> {
    pub mode: Res<'w, GameMode>,
    pub altar: Res<'w, gd_game_core::ascendant::AltarState>,
}

impl Session<'_> {
    pub fn mode(&self) -> gd_rules::mode::Mode {
        self.mode.0
    }

    pub fn ascendant(&self) -> &gd_sim::ascendant::Ascendant {
        &self.altar.formulas
    }

    /// Loot parameters with the session's mode (its difficulty) and altar modifiers.
    pub fn loot(&self, parent_level: u32, player_level: u32) -> gd_sim::items::LootParams {
        let mode = self.mode.0;
        gd_sim::items::LootParams::new(parent_level, player_level, mode.difficulty as u32)
            .with_session(mode, &self.altar.formulas)
    }
}

/// Mounts the session's mod (`Engine::InitializeMod`, or `None`: `RemoveModDirectory`) and drops the caches
/// built from the other data, render caches included ([`gd_game_core::data::ModCacheDrop`]). No-op when that
/// mod is already mounted.
pub fn set_mod_data(world: &mut World, name: Option<&str>) -> anyhow::Result<()> {
    let data = world.resource::<gd_game_core::data::GameData>().clone();
    if data.active_mod() == name {
        return Ok(());
    }
    let mounted = data.set_mod(name);
    if let Some(mut p) = world.get_resource_mut::<crate::monsters::ProfileCache>() {
        p.0.clear();
    }
    if let Some(mut b) = world.get_resource_mut::<gd_game_core::creature::BodyCache>() {
        *b = Default::default();
    }
    if let Some(mut a) = world.get_resource_mut::<gd_game_core::character::AnimCache>() {
        *a = Default::default();
    }
    if let Some(drop) = world.get_resource::<gd_game_core::data::ModCacheDrop>() {
        let id = drop.0;
        world.run_system(id)?;
    }
    mounted
}

/// Starts the session: `difficulty` (0..=2) becomes the character's last played difficulty.
pub fn begin(
    commands: &mut Commands,
    mut choice: Choice,
    gdc: Option<Gdc>,
    difficulty: usize,
    veteran: bool,
) {
    // No challenge in the Crucible (`GameModeWindow::Accept` / `DifficultyWindow::ApplySelection` set
    // `GameInfo::SetChallenge(0)` in mode 1), whatever the character last played.
    let veteran = veteran && choice.survival.is_none();
    let mode = gd_rules::mode::Mode::new(difficulty, veteran);
    let (veteran, ultimate_veteran) = (mode.veteran(), mode.ultimate_veteran());
    // The mod is mounted before anything of the session reads it. A mod that does not mount aborts the
    // start; a headless start (`skip_menu`) exits the process.
    let mod_name = (!choice.mod_name().is_empty()).then(|| choice.mod_name().to_owned());
    commands.queue(move |world: &mut World| {
        // The menu's save/mod refusal (`Choice::mod_mismatch`), for starts that bypass it.
        if let Some(msg) = choice.mod_mismatch(difficulty) {
            error!("session: {} does not start: {msg}", choice.name);
            #[cfg(feature = "dev")]
            if world
                .resource::<gd_game_core::hooks::Hooks>()
                .session
                .skip_menu
            {
                world.write_message(AppExit::error());
            }
            return;
        }
        if let Err(e) = set_mod_data(world, mod_name.as_deref()) {
            error!("session: mod {mod_name:?} does not mount, start aborted: {e:#}");
            #[cfg(feature = "dev")]
            if world
                .resource::<gd_game_core::hooks::Hooks>()
                .session
                .skip_menu
            {
                world.write_message(AppExit::error());
            }
            return;
        }
        world.insert_resource(GameMode(mode));
        if let Some(mut g) = gdc {
            g.info.last_difficulty = last_difficulty(mode, &choice);
            choice.hardcore = g.header.hardcore != 0;
            info!(
                "session: {:?} level {} ({}), difficulty {}{}",
                g.header.name,
                g.bio.level,
                g.header.class_tag,
                g.info.difficulty(),
                if veteran {
                    " (Veteran)"
                } else if ultimate_veteran {
                    " (Ultimate Veteran)"
                } else {
                    ""
                }
            );
            world.insert_resource(LoadedCharacter(Arc::new(g)));
            world.insert_resource(crate::save::AutoSave::default());
        } else {
            info!(
                "session: new character {:?}{}",
                choice.name,
                if choice.female { " (female)" } else { "" }
            );
            world.insert_resource(crate::save::NewCharacter::roll(&choice));
            // The engine writes a new character at creation.
            world.insert_resource(crate::save::AutoSave::created());
        }
        world.insert_resource(choice);
        // The exit's baseline: nothing of the session exists yet.
        let alive = world.iter_entities().map(|e| e.id()).collect();
        world.insert_resource(gd_game_core::session::SessionBaseline(alive));
        world.insert_resource(SessionReady);
    });
}

/// Ends the session's gameplay state (gd-game's part of Exit to Main Menu): session-created resources are
/// removed, startup resources that hold session state go back to `Default` — monster profiles included, as
/// they are built from the session's `CombatRules`.
pub fn end_session_state(world: &mut World) {
    if let Err(e) = set_mod_data(world, None) {
        error!("session: {e:#}");
    }
    world.insert_resource(crate::monsters::ProfileCache::default());
    world.remove_resource::<gd_game_core::session::SessionReady>();
    world.remove_resource::<gd_game_core::session::LoadedCharacter>();
    world.remove_resource::<crate::save::NewCharacter>();
    world.remove_resource::<crate::save::AutoSave>();
    world.remove_resource::<gd_game_core::session::PlayerStart>();
    world.remove_resource::<gd_game_core::session::DifficultyUnlock>();
    world.remove_resource::<crate::combo::ComboRules>();
    world.remove_resource::<crate::devotion::DevotionState>();
    world.remove_resource::<crate::factions::PlayerFactions>();
    world.remove_resource::<crate::fog::FogOfWar>();
    world.remove_resource::<crate::illusion::Illusions>();
    world.remove_resource::<gd_game_core::inventory::Inventory>();
    world.remove_resource::<crate::loot_filter::LootFilter>();
    world.remove_resource::<crate::lore::LoreCodex>();
    world.remove_resource::<gd_game_core::progress::PlayerProgress>();
    world.remove_resource::<crate::reclamation::SkillReclamation>();
    world.remove_resource::<crate::riftgates::TravelState>();
    world.remove_resource::<crate::riftgates::WaypointIndex>();
    world.remove_resource::<crate::riftgates::RiftgateGroup>();
    world.remove_resource::<gd_game_core::rules::CombatRules>();
    world.remove_resource::<gd_game_core::rules::RulesFailed>();
    world.remove_resource::<gd_game_core::stash::Stash>();
    world.remove_resource::<gd_game_core::story::StorySetUp>();
    world.remove_resource::<crate::tutorials::TutorialPages>();
    world.remove_resource::<gd_game_core::entry::EnteredGame>();
    world.insert_non_send(crate::story_world::StoryWorld::default());
    world.insert_resource(crate::story_world::SpawnedActors::default());
    world.insert_resource(gd_game_core::day_clock::DayClock::default());
    world.insert_resource(gd_game_core::session::Choice::default());
    world.insert_resource(gd_game_core::session::GameMode::default());
    world.insert_resource(crate::nav::Nav::default());
    world.insert_resource(crate::collision::WorldCollision::default());
    world.insert_resource(gd_game_core::world::ChestStates::default());
    world.insert_resource(crate::doors::DoorStates::default());
    world.insert_resource(crate::save::BoxedItems::default());
    world.insert_resource(crate::patrol::PatrolGroups::default());
    world.insert_resource(crate::monsters::ProxyStates::default());
    world.insert_resource(crate::endless::ShrineMemory::default());
    world.insert_resource(crate::monsters::UnifiedPools::default());
    world.insert_resource(crate::monsters::SetPieceStates::default());
    world.insert_resource(crate::play_stats::PlayCounts::default());
    world.insert_resource(crate::riftgates::PersonalPortal::default());
    world.insert_resource(crate::unique::UniqueSelection::default());
    world.insert_resource(crate::loot::DroppedEpics::default());
    world.insert_resource(crate::loot::LevelItems::default());
    world.insert_resource(gd_game_core::ascendant::AltarState::default());
    world.insert_resource(crate::hotbar::HeldHotSlots::default());
    world.insert_resource(crate::hotbar::ArmedSlot::default());
    world.insert_resource(crate::hotbar::WorldCursor::default());
    world.insert_resource(crate::hotbar::NewMouseCommand::default());
    world.insert_resource(crate::sheet_dps::SheetDpsValues::default());
    world.insert_resource(crate::riftgates::RiftgateMap::default());
    world.insert_resource(crate::devotion::DevotionUndo::default());
    world.insert_resource(crate::devotion::ShrineWindow::default());
    world.insert_resource(crate::points::SkillUndo::default());
    world.insert_resource(crate::ascendant::DreadCount::default());
    world.insert_resource(crate::nemesis::Nemeses::default());
    // `GameEngine::ClearAllChallengeAreas` (`ExitPlayingMode`).
    world.insert_resource(crate::challenge_areas::ChallengeAreas::default());
    // `TickManager::Reset`.
    world.insert_resource(crate::damage_sectors::TickManager::default());
    world.insert_resource(crate::synergy::TeamPool::default());
    world.insert_resource(crate::graeae::FirstFire::default());
    world.insert_resource(crate::conversation::Conversation::default());
    world.insert_resource(crate::cauldron::PotionWindow::default());
    world.insert_resource(gd_game_core::world::Broken::default());
    world.insert_resource(crate::trade::MarketTimers::default());
    world.insert_resource(crate::trade::Markets::default());
    world.insert_resource(crate::trade::ServiceVisit::default());
    world.insert_resource(gd_game_core::pause::GamePause::default());
}

/// Starts the session the hooks describe without the menu (`skip_menu`).
#[cfg(feature = "dev")]
pub fn load_save(
    mut commands: Commands,
    hooks: Res<gd_game_core::hooks::Hooks>,
    epoch: Res<SessionEpoch>,
) {
    let hooks = &hooks.session;
    if !hooks.skip_menu {
        return;
    }
    let new = Choice {
        save: None,
        name: hooks.name.clone().unwrap_or_else(|| "Player".into()),
        female: hooks.female,
        hardcore: hooks.hardcore,
        // Only the process's first session (a restart after an exit is the campaign).
        survival: hooks
            .survival
            .as_deref()
            .filter(|_| epoch.0 == 0)
            .map(arena_map),
        custom: hooks.module.as_ref().map(|module| CustomMap {
            module: module.clone(),
            level: hooks
                .mod_map
                .clone()
                .unwrap_or_else(|| "levels/world001.map".to_owned()),
        }),
    };
    let Some(path) = hooks.save.clone() else {
        begin(&mut commands, new, None, 0, false);
        return;
    };
    match std::fs::read(&path)
        .map_err(anyhow::Error::from)
        .and_then(|b| Gdc::parse(&b))
    {
        Ok(gdc) => {
            info!("loaded character {:?} from {path}", gdc.header.name);
            // `GD_DIFFICULTY` / `GD_VETERAN` override the save's.
            let (d, vet) = mode_for(&gdc);
            let (d, vet) = (hooks.difficulty.unwrap_or(d), hooks.veteran.unwrap_or(vet));
            begin(
                &mut commands,
                Choice {
                    save: Some(path.into()),
                    ..new
                },
                Some(gdc),
                d,
                vet,
            );
        }
        Err(e) => {
            error!("GD_SAVE {path}: {e:#}");
            begin(&mut commands, new, None, 0, false);
        }
    }
}
