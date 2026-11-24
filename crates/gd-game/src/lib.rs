//! Headless gameplay: the game's state and rules as Bevy ECS components,
//! resources and systems, with no rendering, windows, audio or input.
//!
//! Layering and the rules (one source per value, UI only reads and sends
//! requests): `docs/architecture.md`.
//!
//! [`GameLogicPlugin`] adds everything that runs headless; tests build an
//! `App` with `MinimalPlugins` + this plugin (see [`testing`]).
// Bevy system parameters (queries with filters, param tuples) are long by
// nature; Bevy recommends allowing this lint for them.
#![allow(clippy::type_complexity)]
pub mod achievements;
pub mod afflictions;
pub mod anger_rolls;
pub mod ascendant;
pub mod auras;
pub mod autocast;
pub mod breakables;
pub mod buff_bar;
pub mod casting;
pub mod cauldron;
pub mod challenge_areas;
pub mod char_sounds;
pub mod character;
pub mod chests;
pub mod collision;
pub mod colossus;
pub mod combat;
pub mod combat_sounds;
pub mod combo;
pub mod compass;
pub mod constitution;
pub mod consumables;
pub mod controller_target;
pub mod conversation;
pub mod crafting;
pub mod creature;
pub mod damage_sectors;
pub mod data;
pub mod day_clock;
pub mod death;
pub mod devotion;
pub mod dodge;
pub mod doors;
pub mod endless;
pub mod entry;
pub mod error_vox;
pub mod evade;
pub mod factions;
pub mod flee;
pub mod floating;
pub mod fog;
pub mod geysers;
pub mod graeae;
pub mod hotbar;
pub mod illusion;
pub mod inventory;
pub mod item_drop;
pub mod lead;
pub mod levels;
pub mod line_fan;
pub mod loot;
pub mod loot_filter;
pub mod lore;
pub mod melee;
pub mod monster_skills;
pub mod monsters;
pub mod movement;
pub mod nav;
pub mod nemesis;
pub mod notify;
pub mod npc_controller;
pub mod npc_marks;
pub mod obstacles;
pub mod orbit;
pub mod patrol;
pub mod patterns;
pub mod pause;
pub mod pets;
pub mod physics_decoration;
pub mod play_stats;
pub mod player;
pub mod player_orders;
pub mod points;
pub mod progress;
pub mod quest_reward;
pub mod ragdoll;
pub mod reclamation;
pub mod reflect_link;
pub mod refresh;
pub mod reroll;
pub mod riftgates;
pub mod rules;
pub mod save;
pub mod secondary;
pub mod session;
pub mod shapeshift;
pub mod sheet_dps;
pub mod skill_buffs;
pub mod skill_fx;
pub mod spirit;
pub mod stack;
pub mod stash;
pub mod story;
pub mod story_world;
pub mod survival;
pub mod synergy;
pub mod teleport;
#[cfg(feature = "testing")]
pub mod testing;
pub mod trade;
pub mod turning;
pub mod turrets;
pub mod tutorials;
pub mod unique;
pub mod visuals;
pub mod waves;
pub mod weapon_pool;
pub mod weapon_sets;

use bevy::prelude::*;

/// All headless gameplay systems. Expects a [`gd_game_core::data::GameData`] resource
/// (the game adds [`data::DataPlugin`]; tests insert one).
pub struct GameLogicPlugin;

impl Plugin for GameLogicPlugin {
    fn build(&self, app: &mut App) {
        // `Hooks` first: `SaveDir` (`FromWorld`) reads it; the game inserts the process's before this plugin.
        app.init_resource::<gd_game_core::hooks::Hooks>()
            .init_resource::<gd_game_core::session::SessionEpoch>()
            .init_resource::<gd_game_core::session::SaveDir>()
            .init_resource::<nav::Nav>()
            .init_resource::<gd_game_core::session::Choice>()
            .init_resource::<gd_game_core::session::GameMode>()
            .init_resource::<loot::DroppedEpics>()
            .init_resource::<gd_game_core::levels::Levels>()
            .init_resource::<gd_game_core::creature::BodyCache>()
            .init_resource::<monsters::ProxyStates>()
            .init_resource::<monsters::UnifiedPools>()
            .init_resource::<monsters::ProfileCache>()
            .init_resource::<collision::WorldCollision>()
            .init_resource::<gd_game_core::options::GameOptions>()
            .add_systems(First, creature::collect_body_loads)
            .add_systems(Update, nav::sync_obstacles)
            .add_systems(
                Last,
                (
                    levels::place_loose,
                    levels::despawn_unloaded,
                    nav::settle_connectivity,
                ),
            )
            .add_plugins((
                rules::RulesPlugin,
                character::CharacterPlugin,
                combat::CombatPlugin,
                story::StoryEffectsPlugin,
                player::PlayerStatePlugin,
                player_orders::PlayerOrdersPlugin,
                progress::ProgressPlugin,
                inventory::InventoryPlugin,
                points::PointsPlugin,
                movement::MovementPlugin,
                factions::FactionsPlugin,
                visuals::VisualsPlugin,
            ))
            .add_plugins((
                casting::CastingPlugin,
                pets::PetsPlugin,
                monster_skills::MonsterSkillsPlugin,
                afflictions::AfflictionsPlugin,
                combat::cc_states::CcStatesPlugin,
                melee::MeleePlugin,
                patrol::PatrolPlugin,
                weapon_pool::WeaponPoolPlugin,
                secondary::SecondaryPlugin,
                shapeshift::ShapeshiftPlugin,
                autocast::AutocastPlugin,
                sheet_dps::SheetDpsPlugin,
                evade::EvadePlugin,
                monsters::MonstersPlugin,
                loot::LootPlugin,
            ))
            .add_plugins((
                death::DeathPlugin,
                chests::ChestsPlugin,
                breakables::BreakablesPlugin,
                (
                    physics_decoration::PhysicsDecorationPlugin,
                    ragdoll::RagdollPlugin,
                ),
                doors::DoorsPlugin,
                devotion::DevotionRequestsPlugin,
                devotion::DevotionPlugin,
                (
                    stash::StashPlugin,
                    stack::StackPlugin,
                    item_drop::ItemDropPlugin,
                ),
                (
                    riftgates::RiftgatesPlugin,
                    unique::UniquePlugin,
                    compass::CompassPlugin,
                    quest_reward::QuestRewardPlugin,
                ),
                (
                    consumables::ConsumablesPlugin,
                    constitution::ConstitutionPlugin,
                ),
                (
                    nemesis::NemesisPlugin,
                    challenge_areas::ChallengeAreasPlugin,
                    damage_sectors::DamageSectorsPlugin,
                ),
                (
                    trade::TradePlugin,
                    (story::StoryPlugin, story_world::StoryWorldPlugin),
                    conversation::ConversationPlugin,
                    (
                        ascendant::AscendantPlugin,
                        crafting::CraftingPlugin,
                        reroll::RerollPlugin,
                    ),
                    save::SavePlugin,
                ),
            ))
            .add_plugins((
                (
                    cauldron::CauldronPlugin,
                    reclamation::ReclamationPlugin,
                    entry::EntryPlugin,
                    day_clock::DayClockPlugin,
                ),
                hotbar::HotbarPlugin,
                lore::LorePlugin,
                floating::FloatingTextPlugin,
                pause::PausePlugin,
                weapon_sets::WeaponSetsPlugin,
                skill_fx::SkillFxPlugin,
                combo::ComboPlugin,
                tutorials::TutorialsPlugin,
                combat_sounds::CombatSoundsPlugin,
                skill_buffs::SkillBuffsPlugin,
                refresh::RefreshPlugin,
                waves::WavesPlugin,
                line_fan::LineFanPlugin,
                reflect_link::ReflectLinkPlugin,
            ))
            .add_plugins((
                achievements::AchievementsPlugin,
                colossus::ColossusPlugin,
                survival::SurvivalPlugin,
                endless::EndlessPlugin,
                geysers::GeysersPlugin,
                turrets::TurretsPlugin,
                patterns::PatternsPlugin,
                graeae::GraeaePlugin,
                synergy::SynergyPlugin,
                spirit::SpiritPlugin,
            ))
            .add_plugins((
                orbit::OrbitPlugin,
                npc_marks::NpcMarksPlugin,
                error_vox::ErrorVoxPlugin,
                play_stats::PlayStatsPlugin,
                fog::FogPlugin,
            ))
            .add_systems(Update, story::game_won);
    }
}
