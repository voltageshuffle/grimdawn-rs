//! Skill point reclamation at a spirit guide (`NpcSkillReallocator`): the skills window in reclamation
//! mode, where a click on a skill takes one point back for iron bits (rules: `gd_sim::reclamation`).
//!
//! * Reach: open at once within player radius + NPC radius + 3 m, else at the walk's end.
//! * Stock data: the spirit guides are `records/creatures/npcs/merchants/*reallocator*.dbr`; the player
//!   record holds the price table.
//! * Opening (`NpcSkillReallocator::OnPlayerInteract` → `GameUI::DisplaySkillReallocationWindow`, then the
//!   NPC says hello): with no mastery and no devotion point spent it shows `tagReclaimNoPoints` and opens
//!   nothing; otherwise it closes the open windows; with a mastery the skills window opens in reclamation
//!   mode for that NPC (tutorial page 35 unlocked on show), else the devotion window opens in its reclaim
//!   mode (`DevotionPane.reclaimMode`, recomputing the star locks).
//! * Switching ([`ReclaimRequest::Switch`]): the skills window's Devotion tab calls
//!   `GameUI::DisplayDevotionReallocationWindow` (no devotion point spent → `tagReclaimNoPoints`, nothing
//!   changes; else the skills window hides and the devotion window opens in its mode); the devotion
//!   window's skills tabs call `DisplaySkillReallocationWindow` again. [`SkillReclamation::devotion`] says
//!   which window has the mode.
//! * The devotion window in its mode: a left click on a taken star refunds it for gold and aether; nothing
//!   takes a star.
//! * Closing (`SkillsWindow::Update`): beyond the NPC's `GetUICloseDistance` (3D) the window hides; hiding
//!   leaves the mode, says goodbye (`SayGoodbye`) and autosaves. The pane's `OnHide` makes the refunds final.
//! * Refund (`UISkillPane::OnButtonClick` in the mode): not a mastery at level 1, the level goes down,
//!   `UseReclamationPoints(1)` charges the price (short of money: the level goes back up), one skill point
//!   comes back and its pane's undo record takes it. A disabled button takes no click.
//! * Saved: the refund count, save block 8 `skill_reclamation_points_used`.
use crate::points::{PlayerRequests, SkillUndo};
use bevy::prelude::*;
use gd_game_core::character::Character;
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player, ProfileDirty};
use gd_game_core::progress::PlayerProgress;
use gd_game_core::session::LoadedCharacter;

pub struct ReclamationPlugin;

impl Plugin for ReclamationPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ReclaimRequest>()
            .add_message::<ReclaimAnswer>()
            .add_systems(Update, init_reclamation)
            .add_systems(
                Update,
                (reach_spirit_guide, run_reclamation)
                    .chain()
                    .in_set(PlayerRequests)
                    .run_if(ready),
            );
    }
}

/// [`run_reclamation`]'s gate.
fn ready(state: Option<Res<SkillReclamation>>, progress: Option<Res<PlayerProgress>>) -> bool {
    state.is_some() && progress.is_some()
}

/// The interact order's spirit guide in reach: [`ReclaimRequest::Open`]
/// (`NpcSkillReallocator::OnPlayerInteract`).
#[allow(clippy::type_complexity)]
fn reach_spirit_guide(
    data: Res<GameData>,
    npcs: Query<(&GlobalTransform, &Character), Without<Player>>,
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
        ),
        With<Player>,
    >,
    mut out: MessageWriter<ReclaimRequest>,
) {
    let Ok((ptf, mut path, mut interact, pcb)) = players.single_mut() else {
        return;
    };
    let Some(e) = interact.0 else { return };
    let Ok((gt, ch)) = npcs.get(e) else { return };
    let rec = data.db.get(&ch.record);
    if !rec.is_some_and(is_spirit_guide) {
        return;
    }
    let d = gt.translation().distance(ptf.translation);
    let npc_radius = gd_game_core::combat::get_radius(rec, gd_game_core::combat::ACTOR_RADIUS);
    if !crate::conversation::in_talk_range(d, pcb.radius, npc_radius) && !path.points.is_empty() {
        return;
    }
    interact.0 = None;
    path.points.clear();
    out.write(ReclaimRequest::Open(e));
}

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct SkillReclamation {
    /// `SkillManager.reclamationPointsUsed`; prices the next refund.
    pub used: u32,
    /// The spirit guide of the open mode (`None`: not in the mode).
    pub npc: Option<Entity>,
    /// The mode is the devotion window's (`DevotionPane.reclaimMode`), not the skills window's.
    pub devotion: bool,
}

impl SkillReclamation {
    pub fn active(&self) -> bool {
        self.npc.is_some()
    }

    pub fn skills_mode(&self) -> bool {
        self.active() && !self.devotion
    }

    pub fn devotion_mode(&self) -> bool {
        self.active() && self.devotion
    }
}

#[derive(Message, Clone, Debug, PartialEq)]
pub enum ReclaimRequest {
    /// `NpcSkillReallocator::OnPlayerInteract`.
    Open(Entity),
    /// The window in the mode hid: leave the mode.
    Close,
    /// The window's tab to the other window in the mode: to the devotion window (`devotion`) or back.
    Switch { devotion: bool },
    /// A click on a skill button of `tree` in the mode.
    Refund { tree: String, skill: String },
}

/// What [`ReclaimRequest::Open`] opened.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReclaimAnswer {
    /// A mastery: the skills window in reclamation mode.
    Skills,
    /// No mastery (or its tab), devotion points spent.
    Devotion,
    /// `tagReclaimNoPoints`.
    NoPoints,
    /// The mode of the skills window (`devotion` false) or the devotion window ended.
    Closed { devotion: bool },
}

pub fn is_spirit_guide(rec: &gd_formats::arz::Record) -> bool {
    rec.class == "NpcSkillReallocator"
}

/// Creates [`SkillReclamation`] once the player exists.
pub fn init_reclamation(
    mut commands: Commands,
    // Option: `state` is the existence check; `loaded` is absent for a new character.
    state: Option<Res<SkillReclamation>>,
    loaded: Option<Res<LoadedCharacter>>,
    players: Query<(), With<Player>>,
) {
    if state.is_some() || players.is_empty() {
        return;
    }
    commands.insert_resource(SkillReclamation {
        used: loaded.map_or(0, |c| c.0.skills.skill_reclamation_points_used),
        npc: None,
        devotion: false,
    });
}

#[allow(clippy::too_many_arguments)]
pub fn flags_for(
    data: &gd_game_core::data::GameData,
    loadout: &Loadout,
    tree: &str,
    skill: &str,
    gold: u64,
    used: u32,
    player_level: i32,
    devotion: Option<&crate::devotion::DevotionState>,
) -> gd_sim::reclamation::Flags {
    let trees = data.skill_trees();
    let full = gd_sim::skills::masteries_allowed(player_level) <= trees.masteries(&loadout.skills);
    let bound = devotion.is_some_and(|d| {
        d.bindings
            .iter()
            .any(|b| b.skill.eq_ignore_ascii_case(skill))
    });
    let Some(t) = trees.tree(tree) else {
        return gd_sim::reclamation::Flags::default();
    };
    gd_sim::reclamation::flags(
        &data.db,
        &loadout.skills,
        &trees,
        t,
        skill,
        gold,
        used,
        full,
        bound,
    )
}

fn spent(
    data: &gd_game_core::data::GameData,
    loadout: &Loadout,
    devotion: Option<&crate::devotion::DevotionState>,
) -> (usize, u32) {
    (
        data.skill_trees().masteries(&loadout.skills),
        devotion.map_or(0, |d| d.total.saturating_sub(d.points)),
    )
}

/// Leaves the mode (answers [`ReclaimAnswer::Closed`], says goodbye and autosaves when it was on).
fn close(
    state: &mut SkillReclamation,
    answers: &mut MessageWriter<ReclaimAnswer>,
    saves: &mut MessageWriter<crate::save::SaveRequest>,
    says: &mut MessageWriter<crate::trade::NpcSay>,
) {
    if let Some(npc) = state.npc.take() {
        says.write(crate::trade::NpcSay::Goodbye(npc));
        info!("reclamation: mode closed");
        answers.write(ReclaimAnswer::Closed {
            devotion: std::mem::take(&mut state.devotion),
        });
        saves.write(crate::save::SaveRequest);
    }
}

/// `GameUI::DisplaySkillReallocationWindow` (`devotion` false) or
/// `GameUI::DisplayDevotionReallocationWindow` (`devotion`) for `npc`.
fn open(
    state: &mut SkillReclamation,
    npc: Entity,
    devotion: bool,
    (masteries, devotion_spent): (usize, u32),
    answers: &mut MessageWriter<ReclaimAnswer>,
    saves: &mut MessageWriter<crate::save::SaveRequest>,
    says: &mut MessageWriter<crate::trade::NpcSay>,
) {
    let to_devotion = devotion || masteries == 0;
    if to_devotion && devotion_spent == 0 {
        info!("reclamation: tagReclaimNoPoints");
        answers.write(ReclaimAnswer::NoPoints);
        return;
    }
    // The window that had the mode closes first.
    if let Some(old) = state.npc.filter(|_| state.devotion != to_devotion) {
        saves.write(crate::save::SaveRequest);
        says.write(crate::trade::NpcSay::Goodbye(old));
    }
    state.npc = Some(npc);
    state.devotion = to_devotion;
    if to_devotion {
        info!("reclamation: devotion window ({devotion_spent} points spent)");
        answers.write(ReclaimAnswer::Devotion);
    } else {
        info!("reclamation: skills window ({} refunds so far)", state.used);
        answers.write(ReclaimAnswer::Skills);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_reclamation(
    mut commands: Commands,
    data: Res<GameData>,
    mut requests: MessageReader<ReclaimRequest>,
    mut answers: MessageWriter<ReclaimAnswer>,
    mut saves: MessageWriter<crate::save::SaveRequest>,
    mut state: ResMut<SkillReclamation>,
    mut progress: ResMut<PlayerProgress>,
    mut undo: ResMut<SkillUndo>,
    // Option: a session without devotion data has no spent points.
    devotion: Option<Res<crate::devotion::DevotionState>>,
    mut players: Query<(Entity, &Transform, &mut Loadout), With<Player>>,
    npcs: Query<(&GlobalTransform, &Character), Without<Player>>,
    mut says: MessageWriter<crate::trade::NpcSay>,
) {
    let Ok((player, ptf, mut loadout)) = players.single_mut() else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        match r {
            ReclaimRequest::Open(npc) => {
                if !npcs
                    .get(*npc)
                    .is_ok_and(|(_, c)| data.db.get(&c.record).is_some_and(is_spirit_guide))
                {
                    continue;
                }
                let spent = spent(&data, &loadout, devotion.as_deref());
                open(
                    &mut state,
                    *npc,
                    false,
                    spent,
                    &mut answers,
                    &mut saves,
                    &mut says,
                );
                // `OnPlayerInteract` says hello after the UI call, whatever it opened.
                says.write(crate::trade::NpcSay::Hello(*npc));
            }
            ReclaimRequest::Switch { devotion: to } => {
                if let Some(npc) = state.npc {
                    let spent = spent(&data, &loadout, devotion.as_deref());
                    open(
                        &mut state,
                        npc,
                        *to,
                        spent,
                        &mut answers,
                        &mut saves,
                        &mut says,
                    );
                }
            }
            ReclaimRequest::Close => close(&mut state, &mut answers, &mut saves, &mut says),
            ReclaimRequest::Refund { tree, skill } if state.skills_mode() => {
                let f = flags_for(
                    &data,
                    &loadout,
                    tree,
                    skill,
                    progress.gold,
                    state.used,
                    progress.progress.level,
                    devotion.as_deref(),
                );
                if f.disabled() {
                    info!("reclamation: cannot refund {skill}: {f:?}");
                    continue;
                }
                let cost = gd_sim::reclamation::charge(&data.db, state.used, 1);
                if progress.gold < cost {
                    continue;
                }
                progress.gold -= cost;
                state.used += 1;
                loadout.skills.lower(skill, 1);
                progress.progress.skill_points += 1;
                undo.record(tree, skill, -1);
                undo.refund(tree);
                info!(
                    "reclamation: {skill} -> {} for {cost} iron bits ({} refunds)",
                    loadout.skills.level(skill),
                    state.used
                );
                commands.entity(player).try_insert(ProfileDirty);
            }
            ReclaimRequest::Refund { .. } => {}
        }
    }
    if let Some(npc) = state.npc {
        let far = npcs.get(npc).map_or(true, |(gt, c)| {
            gt.translation().distance(ptf.translation)
                > crate::conversation::close_distance(data.db.get(&c.record))
        });
        if far {
            close(&mut state, &mut answers, &mut saves, &mut says);
        }
    }
}
