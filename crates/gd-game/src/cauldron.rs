//! The potions window ("Cauldron"): choosing each potion's container and its
//! two modifiers. This module owns whether the window is open
//! ([`PotionWindow`]) and applies the player's [`PotionRequest`]s with the
//! rules in `gd_sim::potions` (lists, slots, choices); the game draws the
//! window (`grimdawn::ui::potions`) from [`PotionWindow`] and the player's
//! skill book.
//!
//! * Opening: Game.dll `NpcCauldron::OnPlayerInteract` (the
//!   cauldrons `records/creatures/npcs/merchants/areah_cauldron*.dbr`, class
//!   `NpcCauldron`, needs the third expansion): the main player's interaction
//!   asks the UI to open the window for that cauldron and the cauldron says
//!   hello (`Npc::SayHello`); the window shows the page last shown (health
//!   first). Not the HUD potion slots. The interaction is the player's
//!   `PlayerOrder::Interact` (a click, the Interact key), which walks to it;
//!   [`reach_cauldron`] opens the window at the NPC use range
//!   (`DefaultRequestNpcAction`, as `crate::trade`): within player radius +
//!   cauldron radius + 3 m ([`crate::conversation::in_talk_range`]), else at
//!   the walk's end (`MoveToNpc::EndOfPathReached`).
//! * Placement: no level places a cauldron NPC. Kurn's Hold (Region0K041)
//!   and the Ugdenbog coven (Region0H032) place the ScriptEntities
//!   `scriptentitiesgdx3/areah_cauldron{kurnhold,ugdenbog}_transition_01`
//!   whose `onAddToWorld` (`areahmiscencounters.lua`,
//!   `TokenStateBasedObjectSwap`) creates the `NpcCauldron` record when the
//!   player has `POTIONCRAFTING_UNLOCKED` / `..._COVEN` (given there from the
//!   unlock quest's task state), else `areah_cauldron_placeholder` (a plain
//!   Npc); the quest events' `cauldron*Transition` swap it later. The port
//!   runs these through the story (`gd_sim::story`, story actors).
//! * Closing: GrimDawn.exe window update `sub_140276750`: when the
//!   3D distance from the player to the cauldron exceeds its
//!   `GetUICloseDistance` ([`crate::conversation::close_distance`]); the
//!   close button and Esc (`sub_140276960`: Esc first closes the modifier
//!   list) close it; closing makes the cauldron say goodbye.
//! * Choices: a click on a container that is not on chooses it; a click on a
//!   modifier slot opens the list of the potion's unlocked modifiers and the
//!   pick goes into that slot (`sub_140276E30`, `sub_1402777A0`). Slots take
//!   no choice while the potion has no container
//!   (`gd_sim::potions::slots_enabled`).
//! * Saving (GrimDawn.exe window show handler `sub_1402769b0`):
//!   showing the window calls `GameEngine::SetSaveEnabled(false)`; hiding it
//!   (only when it was shown) makes the cauldron say goodbye, then
//!   `SetSaveEnabled(true)` and `AutoSave`. Here [`save_disabled`] holds the
//!   timed autosave back (`save.rs`) and [`cauldron_autosave`] sends a
//!   `SaveRequest` when the window closes. PORT: F5, the menu's save and the
//!   quit save are the port's own and are not blocked; the cauldron's hello /
//!   goodbye sounds are not played.
//!   [`PotionRequest::Open`] with no cauldron (test hook `GD_UI_OPEN=potions`)
//!   opens it without a distance check.
use crate::character::Character;
use crate::data::GameData;
use crate::player::{Loadout, Player, ProfileDirty};
use bevy::prelude::*;

pub struct CauldronPlugin;

impl Plugin for CauldronPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PotionRequest>()
            .init_resource::<PotionWindow>()
            .add_systems(Update, reach_cauldron.before(crate::player::PlayerSet))
            .add_systems(
                Update,
                (run_window, cauldron_autosave)
                    .chain()
                    .after(crate::player::PlayerSet),
            );
    }
}

/// The player's requests to the potions window.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum PotionRequest {
    /// Open the window at a cauldron (`None`: without one, a test hook).
    Open(Option<Entity>),
    /// Close it (close button, Esc).
    Close,
    /// Show a potion's page: 0 health, 1 energy (the tab buttons).
    Page(usize),
    /// Choose a container (a click on it).
    Container(String),
    /// Put a modifier in a slot (0 or 1) of its potion.
    Modifier { skill: String, slot: usize },
}

/// The open window. `version` changes with every change.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct PotionWindow {
    pub open: bool,
    pub cauldron: Option<Entity>,
    /// Shown potion: 0 health, 1 energy.
    pub page: usize,
    pub version: u64,
}

/// Whether a record is a cauldron.
pub fn is_cauldron(rec: &gd_formats::arz::Record) -> bool {
    rec.class == "NpcCauldron"
}

/// Whether the window is open, which disables saving (`SetSaveEnabled(false)`,
/// see the module doc).
pub fn save_disabled(window: &PotionWindow) -> bool {
    window.open
}

/// Closing the window autosaves (`GameEngine::AutoSave`).
pub fn cauldron_autosave(
    window: Res<PotionWindow>,
    mut was_open: Local<bool>,
    mut fresh: crate::session::NewSession,
    mut save: MessageWriter<crate::save::SaveRequest>,
) {
    // The window of a finished session is gone with it: no close to autosave.
    if fresh.begun() {
        *was_open = false;
    }
    let open = save_disabled(&window);
    if *was_open && !open {
        info!("potions window closed, autosave");
        save.write(crate::save::SaveRequest);
    }
    *was_open = open;
}

fn close(w: &mut PotionWindow) {
    if w.open {
        w.open = false;
        w.cauldron = None;
        w.version += 1;
    }
}

/// The interact order's cauldron target in use range (module doc): takes the target and the walk
/// and asks for the window ([`PotionRequest::Open`]). No other use system routes a cauldron.
pub fn reach_cauldron(
    data: Res<GameData>,
    window: Res<PotionWindow>,
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
            &crate::combat::Combatant,
        ),
        With<Player>,
    >,
    npcs: Query<(&GlobalTransform, &Character), Without<Player>>,
    mut out: MessageWriter<PotionRequest>,
) {
    if window.open {
        return;
    }
    let Ok((ptf, mut path, mut interact, pcb)) = players.single_mut() else {
        return;
    };
    let Some(e) = interact.0 else { return };
    let Ok((gt, ch)) = npcs.get(e) else { return };
    let rec = data.db.get(&ch.record);
    if !rec.is_some_and(is_cauldron) {
        return;
    }
    let d = gt.translation().distance(ptf.translation);
    let npc_radius = crate::combat::get_radius(rec, crate::combat::ACTOR_RADIUS);
    if !crate::conversation::in_talk_range(d, pcb.radius, npc_radius) && !path.points.is_empty() {
        return;
    }
    interact.0 = None;
    path.points.clear();
    out.write(PotionRequest::Open(Some(e)));
}

/// Applies the requests, then closes the window when the player walked away
/// from the cauldron (or it is gone).
pub fn run_window(
    mut commands: Commands,
    data: Res<GameData>,
    mut requests: MessageReader<PotionRequest>,
    mut window: ResMut<PotionWindow>,
    mut players: Query<(Entity, &Transform, &mut Loadout), With<Player>>,
    npcs: Query<(&GlobalTransform, &Character), Without<Player>>,
) {
    let Ok((player, ptf, mut loadout)) = players.single_mut() else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        match r {
            PotionRequest::Open(npc) => {
                if let Some(e) = npc
                    && !npcs
                        .get(*e)
                        .is_ok_and(|(_, c)| data.db.get(&c.record).is_some_and(is_cauldron))
                {
                    continue;
                }
                window.open = true;
                window.cauldron = *npc;
                window.version += 1;
                info!("potions window: open");
            }
            PotionRequest::Close => close(&mut window),
            PotionRequest::Page(p) if window.open && *p < 2 && window.page != *p => {
                window.page = *p;
                window.version += 1;
            }
            PotionRequest::Container(skill) if window.open => {
                match gd_sim::potions::choose_container(&data.db, &mut loadout.skills, skill) {
                    Ok(()) => {
                        info!(
                            "potions window: container {}",
                            skill.rsplit('/').next().unwrap_or(skill)
                        );
                        commands.entity(player).try_insert(ProfileDirty);
                    }
                    Err(e) => info!("potions window: cannot choose {skill}: {e}"),
                }
            }
            PotionRequest::Modifier { skill, slot } if window.open => {
                let trees = gd_sim::potions::trees(&data.db);
                let enabled = trees
                    .iter()
                    .find(|t| t.modifiers.iter().any(|m| m.eq_ignore_ascii_case(skill)))
                    .is_some_and(|t| gd_sim::potions::slots_enabled(&loadout.skills, t));
                if !enabled {
                    info!("potions window: no container chosen for {skill}");
                    continue;
                }
                match gd_sim::potions::place_modifier(&data.db, &mut loadout.skills, skill, *slot) {
                    Ok(()) => {
                        info!(
                            "potions window: modifier {} in slot {slot}",
                            skill.rsplit('/').next().unwrap_or(skill)
                        );
                        commands.entity(player).try_insert(ProfileDirty);
                    }
                    Err(e) => info!("potions window: cannot choose {skill}: {e}"),
                }
            }
            _ => {}
        }
    }
    if let Some(e) = window.cauldron.filter(|_| window.open) {
        let far = npcs.get(e).map_or(true, |(gt, c)| {
            gt.translation().distance(ptf.translation)
                > crate::conversation::close_distance(data.db.get(&c.record))
        });
        if far {
            close(&mut window);
        }
    }
}
