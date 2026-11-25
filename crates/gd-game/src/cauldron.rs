//! The potions window ("Cauldron"): choosing each potion's container and its two modifiers with
//! the rules in `gd_sim::potions`.
//!
//! * Opening (`NpcCauldron::OnPlayerInteract`, third expansion): the player's interaction opens the
//!   window on the page last shown (health first) and the cauldron says hello. [`reach_cauldron`]
//!   opens it at the NPC use range (player radius + cauldron radius + 3 m), else at the walk's end.
//! * Placement: no level places a cauldron NPC; Kurn's Hold and the Ugdenbog coven place
//!   ScriptEntities whose `onAddToWorld` (`TokenStateBasedObjectSwap`) creates the `NpcCauldron`
//!   with `POTIONCRAFTING_UNLOCKED` / `..._COVEN`, else a placeholder Npc; quest events swap it later.
//! * Closing (`PotionsWindow::Update`): past its `GetUICloseDistance` (3D), the close button or Esc
//!   (which first closes the modifier list); the cauldron says goodbye.
//! * Choices (`PotionsWindow::OnButtonPress`, `PlaceModifier`): a click chooses a container; a
//!   modifier slot opens the potion's unlocked modifiers. Slots take no choice without a container.
//! * Saving (`PotionsWindow::Open`): the open window disables saving (`SetSaveEnabled(false)`);
//!   closing re-enables it and autosaves.
//!   PORT: F5, the menu's save and the quit save are not blocked; hello / goodbye sounds are not
//!   played. [`PotionRequest::Open`] with no cauldron opens it without a distance check.
use bevy::prelude::*;
use gd_game_core::character::Character;
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player, ProfileDirty};

pub struct CauldronPlugin;

impl Plugin for CauldronPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PotionRequest>()
            .init_resource::<PotionWindow>()
            .add_systems(
                Update,
                reach_cauldron.before(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                (run_window, cauldron_autosave)
                    .chain()
                    .after(gd_game_core::player::PlayerSet),
            );
    }
}

/// `Open(None)`: without a cauldron (test hook). `Page`: 0 health, 1 energy. `slot`: 0 or 1.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum PotionRequest {
    Open(Option<Entity>),
    Close,
    Page(usize),
    Container(String),
    Modifier { skill: String, slot: usize },
}

/// `version` changes with every change.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct PotionWindow {
    pub open: bool,
    pub cauldron: Option<Entity>,
    /// 0 health, 1 energy.
    pub page: usize,
    pub version: u64,
}

pub fn is_cauldron(rec: &gd_formats::arz::Record) -> bool {
    rec.class == "NpcCauldron"
}

pub fn save_disabled(window: &PotionWindow) -> bool {
    window.open
}

/// `GameEngine::AutoSave` on close.
pub fn cauldron_autosave(
    window: Res<PotionWindow>,
    mut was_open: Local<bool>,
    mut fresh: gd_game_core::session::NewSession,
    mut save: MessageWriter<crate::save::SaveRequest>,
) {
    // A finished session's window is gone with it: no close to autosave.
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

/// The interact order's cauldron target in use range: takes the target and the walk and opens the
/// window. No other use system routes a cauldron.
pub fn reach_cauldron(
    data: Res<GameData>,
    window: Res<PotionWindow>,
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
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
    let npc_radius = gd_game_core::combat::get_radius(rec, gd_game_core::combat::ACTOR_RADIUS);
    if !crate::conversation::in_talk_range(d, pcb.radius, npc_radius) && !path.points.is_empty() {
        return;
    }
    interact.0 = None;
    path.points.clear();
    out.write(PotionRequest::Open(Some(e)));
}

pub fn run_window(
    mut commands: Commands,
    data: Res<GameData>,
    mut requests: MessageReader<PotionRequest>,
    mut window: ResMut<PotionWindow>,
    mut players: Query<(Entity, &Transform, &mut Loadout), With<Player>>,
    npcs: Query<(&GlobalTransform, &Character), Without<Player>>,
    fx: Query<(Entity, &gd_game_core::visuals::BuffFx)>,
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
                        wipe_switched_off(
                            &mut commands,
                            &data,
                            &fx,
                            player,
                            ptf.translation,
                            &mut loadout,
                        );
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
                        wipe_switched_off(
                            &mut commands,
                            &data,
                            &fx,
                            player,
                            ptf.translation,
                            &mut loadout,
                        );
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

/// A potion skill switched out (level back to 0) stops its running buff (`PotionsWindow::StopPotionBuff`).
fn wipe_switched_off(
    commands: &mut Commands,
    data: &GameData,
    fx: &Query<(Entity, &gd_game_core::visuals::BuffFx)>,
    player: Entity,
    at: Vec3,
    loadout: &mut Loadout,
) {
    let off: Vec<String> = gd_sim::potions::trees(&data.db)
        .iter()
        .flat_map(|t| t.containers.iter().chain(&t.modifiers))
        .filter(|s| loadout.skills.level(s) == 0)
        .cloned()
        .collect();
    let gone = loadout
        .buffs
        .stop_where(|b| off.iter().any(|s| s.eq_ignore_ascii_case(b)));
    if !gone.is_empty() {
        crate::skill_buffs::timed_buffs_ended(commands, data, fx, player, at, &gone);
        commands.entity(player).try_insert(ProfileDirty);
    }
}
