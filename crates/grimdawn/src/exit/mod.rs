//! Exit to Main Menu: tears the play session down in the running process.
//!
//! PORT: the engine's `GameEngine::ExitPlayingMode` ends the session in place; this is the port's equivalent.
//! A session leaves three kinds of state:
//! - entities: everything not alive when the session began is despawned (`gd_game_core::session::SessionBaseline`,
//!   taken as `SessionReady` is inserted: a menu start can spawn the player in that same frame);
//! - resources the session created: removed (the `init_*` systems create them again);
//! - resources that exist from startup but hold session state: reset to `Default`
//!   (`Levels`, `StreamedMap` and `StreamFocus` are rebuilt by `gd_world::open_world`).
//!
//! Caches (models, textures, fonts, animations, sounds) stay. System `Local`s cannot be reset from outside:
//! version counters they compare for equality start from [`gd_game_core::session::SessionEpoch`] so a stale
//! one never matches a new session's resource.
use bevy::prelude::*;
use gd_ui::state::exit::ExitToMenu;

pub struct ExitPlugin;

impl Plugin for ExitPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ExitToMenu>()
            .add_systems(Last, end_session.run_if(on_message::<ExitToMenu>));
    }
}

fn reset<T: Resource + FromWorld>(world: &mut World) {
    let fresh = T::from_world(world);
    world.insert_resource(fresh);
}

fn remove<T: Resource>(world: &mut World) {
    world.remove_resource::<T>();
}

fn end_session(world: &mut World) {
    info!("exit to the main menu: tearing the session down");
    let baseline = world
        .remove_resource::<gd_game_core::session::SessionBaseline>()
        .unwrap_or_default()
        .0;
    // Only the roots: despawning a child first queues a hierarchy update on a parent gone by the time it applies.
    let doomed: Vec<Entity> = world
        .iter_entities()
        .filter(|e| !baseline.contains(&e.id()))
        // Resources are entities too: they are removed below, by type.
        .filter(|e| e.get::<bevy::ecs::resource::IsResource>().is_none())
        .filter(|e| {
            e.get::<ChildOf>()
                .is_none_or(|c| baseline.contains(&c.parent()))
        })
        .map(|e| e.id())
        .collect();
    // The menu's streamed levels and doll are in the baseline, but `open_world` forgets the levels and the reset
    // `Dolls` builds new bodies: without this each exit would leave one more copy of both behind.
    let mut doomed = doomed;
    doomed.extend(
        world
            .resource::<gd_game_core::levels::Levels>()
            .loaded
            .values()
            .copied()
            .filter(|e| baseline.contains(e)),
    );
    doomed.extend(
        world
            .resource::<gd_ui::doll::Dolls>()
            .entities()
            .into_iter()
            .filter(|e| baseline.contains(e)),
    );
    for e in doomed {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
    gd_game::session::end_session_state(world);
    remove::<gd_game::compass::CompassMode>(world);
    reset::<gd_ui::state::death::GameOverBox>(world);
    reset::<gd_game::quest_reward::RandomRewardRolls>(world);
    reset::<gd_game::doors::DungeonExits>(world);
    remove::<gd_render::render::env::Ambiance>(world);
    remove::<gd_render::render::shadow::SunShadowSettings>(world);
    remove::<gd_ui::state::crafting::CraftList>(world);
    world.remove_non_send::<gd_game_core::story::StoryRuntime>();
    reset::<gd_ui::state::trade::Trade>(world);
    reset::<crate::casting::MoveKeyHeld>(world);
    reset::<gd_game::movement::MouseCommand>(world);
    reset::<gd_render::decal::DecalReceivers>(world);
    reset::<gd_ui::state::pets::PetSelection>(world);
    reset::<gd_ui::state::ascendant::AltarMenu>(world);
    reset::<crate::doors::PendingTravel>(world);
    reset::<gd_game::teleport::TeleportActivity>(world);
    reset::<gd_world::TravelPreload>(world);
    reset::<gd_world::Lingering>(world);
    reset::<gd_render::render::env::TimeOfDay>(world);
    reset::<gd_render::camera::ShakeState>(world);
    reset::<gd_game::story::ScreenShake>(world);
    reset::<gd_game::monsters::HeroBosses>(world);
    gd_render::camera::reset_cameras(world);
    reset::<gd_ui::state::hover::Hovered>(world);
    reset::<crate::controller_target::ControllerPick>(world);
    reset::<gd_ui::state::loot::HoveredLabel>(world);
    reset::<crate::loot::ListedLabels>(world);
    reset::<gd_ui::state::loot::ItemsHidden>(world);
    reset::<crate::loot::LabelMode>(world);
    reset::<crate::loot::PickMode>(world);
    reset::<gd_audio::Ambience>(world);
    reset::<crate::weather::WeatherRuntime>(world);
    reset::<gd_render::render::weather::WeatherState>(world);
    // Dropping `Playing` stops its decoder thread.
    reset::<crate::video::VideoQueue>(world);
    remove::<crate::video::Playing>(world);
    remove::<gd_render::render::backdrop::MenuBackdrop>(world);
    // The particle decals' ring holds the despawned decals' entities.
    reset::<crate::casting::DropDecals>(world);
    reset::<gd_game::weapon_pool::PoolRng>(world);
    // The listener's voice count and ears describe despawned entities.
    world.resource_mut::<gd_audio::Sounds>().listener = Default::default();
    crate::ui::reset_session(world);
    let _opened = world.run_system_cached(gd_world::open_world);
    world
        .resource_mut::<gd_game_core::session::SessionEpoch>()
        .0 += 1;
    let _menu = world.run_system_cached(crate::menu::setup);
}

/// Resources that survive a session; the only way past `tests::every_registered_resource_is_reset_or_kept`. An
/// entry must not depend on the character, the difficulty or the game mode. Entries are full type paths (a type
/// the scan cannot resolve, one of Bevy's named through its prelude, keeps its bare name).
#[cfg(test)]
const KEEP: &[&str] = &[
    // Lives for the process.
    "gd_game::achievements::Achievements",
    "gd_ui_kit::additive::AdditiveCache",
    "gd_ui_kit::masked::MaskedCache",
    "gd_game_core::character::AnimCache",
    // Dropped when the mounted mod changes.
    "gd_game_core::creature::BodyCache",
    "ClearColor",
    // Stands in for the OS clipboard, which outlives sessions.
    "gd_ui::state::trade::Clipboard",
    // Its `OnEnter(Loading)` takes it again.
    "grimdawn::dev::audit::Baseline",
    "bevy::light::DirectionalLightShadowMap",
    "gd_ui_kit::text::FontCache",
    "gd_render::fx::FxAssets",
    "gd_render::fx::FxStats",
    // Render world: kept in sync from the main world.
    "gd_render::render::gbuffer_blend::GBufferBlendEntities",
    // `end_session_state` unmounts the mod.
    "gd_game_core::data::GameData",
    // Process-lifetime, holds no session data.
    "gd_game_core::data::ModCacheDrop",
    "gd_game_core::options::GameOptions",
    "gd_render::render::GdDefaults",
    // A field's range is freed when its terrain despawns, so it only mirrors the live levels.
    "gd_render::render::grass::GrassOffsets",
    // Render world: refilled at every extract.
    "gd_render::render::grass::ExtractedGrassWrites",
    "GlobalAmbientLight",
    // Render world: items held between two systems of one frame.
    "gd_render::render::distort::HeldDistortion",
    "gd_render::render::post::HeldComposite",
    "gd_render::render::gbuffer_blend::HeldGBufferBlend",
    // Render world: rebuilt every frame.
    "gd_render::water::NoisePipeline",
    "gd_render::water::NoisePasses",
    "gd_game_core::hooks::Hooks",
    "gd_ui_kit::keys::KeyBindings",
    // The install's, read at start-up from the port's settings folder.
    "gd_game::item_style::ItemStyles",
    // Kill counts persist across sessions of the process (`AchievementKill::OnCreatureKilled`).
    "gd_game::achievements::KillCounts",
    // The CRT `rand()` stream of the FireLights: never reseeded.
    "gd_render::render::lights::LightRand",
    // Removed when a session starts, made again by `menu::setup`.
    "grimdawn::menu::MainMenu",
    "grimdawn::menu::MenuLayout",
    // Built while `MainMenu` exists, removed when the menu closes.
    "grimdawn::menu_scene::MenuScene",
    // `model::drop_on_mod_change` empties the caches below when the mounted mod changes.
    "gd_render::model::ModelCache",
    "gd_render::render::material::MaterialCache",
    // Entries are ids only (freed with their last decal).
    "gd_render::decal::DecalMatCache",
    "gd_render::render::grass::GrassQuality",
    "gd_ui_kit::state::settings::UiScaleOption",
    "gd_render::render::post::OutlineGlow",
    // Render world: refilled at every extract.
    "gd_render::render::distort::PostObjectEntities",
    "gd_render::render::post::CompositeEntities",
    "gd_render::render::post::PostPipelines",
    // Render world: pipelines and items held between two systems of one frame.
    "gd_render::fx::half_res::HalfResPipelines",
    "gd_render::fx::half_res::HeldParticles",
    "gd_game_core::session::SaveDir",
    // Dev scenes that place the camera themselves.
    "grimdawn::dev::SceneCamera",
    // `end_session` advances it.
    "gd_game_core::session::SessionEpoch",
    // Brought up to date by every `read_guarded`.
    "gd_game_core::stash::SharedFiles",
    // One screenshot per process.
    "grimdawn::dev::autoshot::Shot",
    "gd_game_core::visuals::ShotTrigger",
    // `end_session` resets its listener by hand.
    "gd_audio::Sounds",
    // The host's; the options window refreshes it.
    "gd_audio::devices::AudioDevices",
    "grimdawn::render_devices::RenderAdapters",
    "grimdawn::dev::chars::TestScene",
    "gd_render::textures::Textures",
    "grimdawn::casting::DecalTextureLoads",
    // Held weakly by the loaded levels; cleared on a mod change.
    "gd_render::render::terrain::TerrainLayers",
    "gd_render::render::post::UiShaders",
    "gd_render::water::WaterMaterials",
    "gd_render::render::reflection::ReflectionTarget",
    // Invariant config.
    "gd_ui::window::WindowPolicy",
    // A cursor into a process-lifetime message queue (a proc left unread names a despawned caster and is skipped).
    "gd_game::casting::ProcCursor",
    // The pipeline cache they warm lives for the process.
    "gd_render::see_through::FadeWarm",
    // Cleared on `ModChanged`.
    "grimdawn::record_facts::RecordFacts",
    // Rewritten every frame from the camera.
    "gd_game::ragdoll::RagdollView",
    // Rewritten every PreUpdate from the app state (`session::sync_state`).
    "gd_render::model::LoadFast",
    // Render world: entries age out after 8 untouched frames; keys own their textures (identity-compared).
    "gd_render::render::post::PostBindGroupCache",
];

#[cfg(test)]
mod tests;
