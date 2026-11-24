//! Exit to Main Menu: tears the play session down in the running process.
//!
//! PORT: the engine's `GameEngine::ExitPlayingMode` ends the session in place (it clears the session's
//! Epics / altar / personal portals, see `gd_game::session::begin`); this is the port's equivalent.
//! A session leaves three kinds of state, all found with the session audit (`GD_CENSUS`, `dev/audit.rs`;
//! `GD_EXIT_CYCLES`, `dev/exit_cycles.rs`, shows what still grows across exits):
//! - entities: everything not alive when `Loading` began is despawned (`Baseline`);
//! - resources the session created: removed (the `init_*` systems create them again);
//! - resources that exist from startup but hold session state: reset to `Default`
//!   (`Levels`, `StreamedMap` and `StreamFocus` are rebuilt by `world::open_world` → `world::install_world`).
//!
//! Caches (models, textures, fonts, animations, sounds) stay; what the ended session alone used leaves them
//! with its last holder (`gd_game_core::weak_cache`), and the freed memory goes back to the system (`heap.rs`). System
//! `Local`s cannot be reset from outside:
//! version counters they compare for equality start from [`gd_game_core::session::SessionEpoch`] so a stale
//! one never matches a new session's resource; the other latches left are test hooks (they do not
//! fire twice) and signature caches that differ from the reset state.
use gd_ui::state::session::AppState;
use gd_ui::state::exit::ExitToMenu;
use bevy::prelude::*;
use gd_formats::collections::HashSet;


pub struct ExitPlugin;

impl Plugin for ExitPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ExitToMenu>()
            .add_systems(OnEnter(AppState::Loading), capture_baseline)
            .add_systems(Last, end_session.run_if(on_message::<ExitToMenu>));
    }
}

/// The entities alive when the session began (cameras, lights, cursor, the persistent UI).
#[derive(Resource, Default)]
struct Baseline(HashSet<Entity>);

fn capture_baseline(world: &mut World) {
    let alive = world.iter_entities().map(|e| e.id()).collect();
    world.insert_resource(Baseline(alive));
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
    // The session's entities (children go with their parents; stale ids are skipped).
    let baseline = world.remove_resource::<Baseline>().unwrap_or_default().0;
    // Only the roots of the doomed set: despawning a parent takes its children along (despawning a
    // child first queues a hierarchy update on a parent that is gone by the time it applies).
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
    // The levels streamed around the menu's start point and the menu's doll are alive when `Loading` begins,
    // so they are in the baseline, but `open_world` forgets the levels (`Levels::new`) and the reset `Dolls`
    // builds new bodies: without this each exit would leave one more copy of both behind. Their roots go, with
    // their trees and the skeletons' joints (`model::Joints`).
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
    // Resources the session created, and the startup ones that hold session state: gd-game's own.
    gd_game::session::end_session_state(world);
    // The same in this crate.
    remove::<gd_game::compass::CompassMode>(world);
    reset::<gd_ui::state::death::GameOverBox>(world);
    reset::<gd_game::quest_reward::RandomRewardRolls>(world);
    // The entrance ↔ exit pairs the player took (`Player.dungeonExits`, freed with the `Player`).
    reset::<gd_game::doors::DungeonExits>(world);
    remove::<gd_render::render::env::Ambiance>(world);
    remove::<gd_render::render::shadow::SunShadowSettings>(world);
    remove::<gd_ui::state::crafting::CraftList>(world);
    world.remove_non_send::<gd_game_core::story::StoryRuntime>();
    // Startup resources that hold session state.
    reset::<gd_ui::state::trade::Trade>(world);
    reset::<crate::casting::MoveKeyHeld>(world);
    reset::<gd_game::movement::MouseCommand>(world);
    reset::<gd_render::decal::DecalReceivers>(world);
    reset::<gd_ui::state::pets::PetSelection>(world);
    reset::<gd_ui::state::ascendant::AltarMenu>(world);
    reset::<crate::doors::PendingTravel>(world);
    reset::<gd_game::teleport::TeleportActivity>(world);
    reset::<crate::world::TravelPreload>(world);
    reset::<crate::world::Lingering>(world);
    reset::<gd_render::render::env::TimeOfDay>(world);
    reset::<gd_render::camera::ShakeState>(world);
    gd_render::camera::reset_cameras(world);
    reset::<gd_ui::state::hover::Hovered>(world);
    reset::<crate::controller_target::ControllerPick>(world);
    reset::<gd_ui::state::loot::HoveredLabel>(world);
    reset::<crate::loot::ListedLabels>(world);
    reset::<gd_ui::state::loot::ItemsHidden>(world);
    reset::<crate::loot::LabelMode>(world);
    reset::<crate::loot::PickMode>(world);
    reset::<crate::audio::Ambience>(world);
    reset::<crate::weather::WeatherRuntime>(world);
    reset::<gd_render::render::weather::WeatherState>(world);
    remove::<gd_render::render::backdrop::MenuBackdrop>(world);
    // The particle decals' ring holds the despawned decals' entities.
    reset::<crate::casting::DropDecals>(world);
    // The weapon pool's roll stream: a new session starts a newly seeded one.
    reset::<gd_game::weapon_pool::PoolRng>(world);
    // The sound cache stays; the listener's voice count and ears describe despawned entities.
    world.resource_mut::<crate::audio::Sounds>().listener = Default::default();
    crate::ui::reset_session(world);
    let _opened = world.run_system_cached(crate::world::open_world);
    world
        .resource_mut::<gd_game_core::session::SessionEpoch>()
        .0 += 1;
    // The main menu again.
    let _menu = world.run_system_cached(crate::menu::setup);
}

/// Resources that deliberately survive a session: caches of loaded data, settings, the process's own state.
/// The only way past `tests::every_registered_resource_is_reset_or_kept`; an entry must not hold anything that depends on
/// the character, the difficulty or the game mode. Entries are full type paths (a type the scan cannot resolve, one of
/// Bevy's named through its prelude, keeps its bare name).
#[cfg(test)]
const KEEP: &[&str] = &[
    // The achievement manager is the game engine's: it lives for the process (`gd_game::achievements`).
    "gd_game::achievements::Achievements",
    "gd_ui_kit::additive::AdditiveCache",
    "gd_game_core::character::AnimCache",
    // Parsed meshes by path (creature bodies and the meshes of render models); `session::set_mod_data` drops them
    // when the mounted mod changes.
    "gd_game_core::creature::BodyCache",
    // Bevy's, set once in main.rs.
    "ClearColor",
    // The edit boxes' clipboard (trade.rs): stands in for the OS clipboard, which outlives sessions.
    "gd_ui::state::trade::Clipboard",
    // `GD_CENSUS`: the entities alive when the census began (dev/audit.rs); its `OnEnter(Loading)` takes it again.
    "grimdawn::dev::audit::Baseline",
    // Bevy's, shadow settings (render/shadow.rs).
    "bevy::light::DirectionalLightShadowMap",
    "gd_ui_kit::text::FontCache",
    "gd_render::fx::FxAssets",
    // `GD_FPS`: the last effect update's time (dev/fx.rs).
    "gd_render::fx::FxStats",
    // Render world: kept in sync from the main world (rebuilt on change, render/gbuffer_blend.rs).
    "gd_render::render::gbuffer_blend::GBufferBlendEntities",
    // The archives and the database; `end_session_state` unmounts the mod (`session::set_mod_data`).
    "gd_game_core::data::GameData",
    "gd_game_core::options::GameOptions",
    // The engine's default textures and environment (render/mod.rs).
    "gd_render::render::GdDefaults",
    // The grass offset fields' shared buffer and its ranges: a field's range is freed by the
    // `grass::release_field` observer when its terrain despawns, so it only mirrors the live levels (render/grass.rs).
    "gd_render::render::grass::GrassOffsets",
    // Render world: refilled from the main world at every extract (render/grass.rs).
    "gd_render::render::grass::ExtractedGrassWrites",
    // Bevy's, set once in main.rs.
    "GlobalAmbientLight",
    // Render world: the distortion items held between two systems of one frame (render/distort.rs).
    "gd_render::render::distort::HeldDistortion",
    // Render world: the glow composite items held between two systems of one frame (render/post.rs).
    "gd_render::render::post::HeldComposite",
    // Render world: the G-buffer blend items held between two systems of one frame (render/gbuffer_blend.rs).
    "gd_render::render::gbuffer_blend::HeldGBufferBlend",
    // The process's test hooks.
    "gd_game_core::hooks::Hooks",
    "gd_ui_kit::keys::KeyBindings",
    // The achievement manager is the game engine's: kill counts persist across sessions of the process
    // (`AchievementKill::OnCreatureKilled`, gd-game achievements.rs).
    "gd_game::achievements::KillCounts",
    // The CRT `rand()` stream of the FireLights (world.rs): the process's, never reseeded.
    "gd_render::render::lights::LightRand",
    // The menu's own: removed when a session starts, made again by `menu::setup` above.
    "grimdawn::menu::MainMenu",
    "grimdawn::menu::MenuLayout",
    // The menu's backdrop: built while `MainMenu` exists, removed when the menu closes (render/menu.rs).
    "grimdawn::menu_scene::MenuScene",
    // Render models by path; `model::drop_on_mod_change` empties it (and `Textures`, `FxAssets`, `Sounds`) when
    // the mounted mod changes.
    "gd_render::model::ModelCache",
    // The .ssh analyses, grass materials and material variants; `model::drop_on_mod_change` empties it.
    "gd_render::render::material::MaterialCache",
    // The `detailLevel` render setting (settings.rs writes it); not session state.
    "gd_render::render::grass::GrassQuality",
    // The UI-scale slider (`settings::apply_ui_scale` writes it from the options and the app state).
    "gd_ui_kit::state::settings::UiScaleOption",
    "gd_render::render::post::OutlineGlow",
    // Render world: refilled from the main world at every extract (render/distort.rs).
    "gd_render::render::distort::PostObjectEntities",
    // Render world: refilled from the main world at every extract (render/post.rs).
    "gd_render::render::post::CompositeEntities",
    // Render pipelines and shader handles (render/post.rs).
    "gd_render::render::post::PostPipelines",
    "gd_game_core::session::SaveDir",
    // Dev scenes (`GD_MODELS`, `GD_CHARS`) that place the camera themselves (dev/mod.rs): no session.
    "grimdawn::dev::SceneCamera",
    // The teardown count itself: `end_session` advances it.
    "gd_game_core::session::SessionEpoch",
    // The shared files this process must not write (unreadable, stash.rs): keyed by the write target,
    // brought up to date by every `read_guarded`, not session state.
    "gd_game_core::stash::SharedFiles",
    // `GD_SHOT` / `GD_SHOT_ON`: one screenshot per process (dev/autoshot.rs, gd-game visuals.rs).
    "grimdawn::dev::autoshot::Shot",
    "gd_game_core::visuals::ShotTrigger",
    // The sound cache; `end_session` resets its listener by hand.
    "grimdawn::audio::Sounds",
    // The OS device list (audio/devices.rs): the host's, not session state; the options window refreshes it.
    "grimdawn::audio::devices::AudioDevices",
    // The GPU adapter list (render_devices.rs): the host's, not session state; the options window refreshes it.
    "grimdawn::render_devices::RenderAdapters",
    // `GD_CHARS`: the character test scene (dev/chars.rs).
    "grimdawn::dev::chars::TestScene",
    "gd_render::textures::Textures",
    // Decal textures decoding in the background, filed into `Textures` when they land (casting.rs).
    "grimdawn::casting::DecalTextureLoads",
    // The decoded terrain layers, held weakly by the loaded levels; cleared on a mod change (render/terrain.rs).
    "gd_render::render::terrain::TerrainLayers",
    "gd_render::render::post::UiShaders",
    "gd_render::water::WaterMaterials",
    "gd_render::render::reflection::ReflectionTarget",
    // The window manager's close-table: invariant config, `Default` builds the same rows every session
    // (ui/window.rs); no session state.
    "gd_ui::window::WindowPolicy",
    // The `ProcCast` read position (casting/mod.rs): a cursor into a process-lifetime message queue, no
    // session data (a proc left unread names a despawned caster and is skipped).
    "gd_game::casting::ProcCursor",
    // The fade pipeline variants already queued for compilation (see_through.rs): the pipeline cache they
    // warm lives for the process; its dummy draws are despawned once in game.
    "gd_render::see_through::FadeWarm",
    // Facts derived from the database's records (record_facts.rs): data, not session state; cleared on `ModChanged`.
    "grimdawn::record_facts::RecordFacts",
    // The game camera's frustum as the ragdoll gate's preload frustum (gd-game ragdoll.rs): rewritten every frame
    // from the camera by `grimdawn::ragdoll`, no session data.
    "gd_game::ragdoll::RagdollView",
];

#[cfg(test)]
mod tests;
