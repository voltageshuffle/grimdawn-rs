//! Spending attribute and skill points ([`SpendAttribute`] / [`RaiseSkill`]): the rules and their effects
//! on the loadout, unspent points and the profile.
//!
//! Skills window undo (`UISkillPane::OnButtonClick` / `Undo` / `OnHide`): every point spent in the window is
//! recorded per pane (mastery tree) and per skill ([`SkillUndo`], `SkillUndoEntry.delta`). A pane's button
//! is enabled while its record is non-empty (`UISkillPane.undoPending`); its undo ([`UndoSkillPoints`])
//! reverts every level recorded in that pane, returns those points and empties that record alone
//! (all-or-nothing, not a stack). It re-checks no requirement; nothing can be orphaned because a skill only
//! requires its own tree's mastery bar. Hiding the window ([`CommitSkillUndo`]) clears every record. Skill
//! points only.
//! In reclamation mode a click refunds a point instead: the record keeps it as a negative entry and counts
//! the refunds (`UISkillPane.refundCount`); undo levels those skills back up, takes the points again and
//! pays the price back (`UseReclamationPoints(-refunds)`). The `StopSkill` call when a level falls to 0 (a
//! no-op on the base `Skill`) is not mirrored.
//!
//! Attribute reset ([`ResetAttributes`], Tonic of Reshaping; `CharacterBio::ResetAttributePoints`): each
//! attribute's base goes back to its initial value and the points spent return to unspent. Refused unless
//! some point is spent (`HasModifierPointsInUse`). The points spent are [`Loadout::allocated`], the save's
//! included, so earlier sessions' points are refunded too.
use crate::inventory::{init_inventory, sync_equipment};
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::inventory::InventorySet;
use gd_game_core::player::{Loadout, Player, ProfileDirty};
use gd_game_core::progress::PlayerProgress;

pub struct PointsPlugin;

impl Plugin for PointsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpendAttribute>()
            .add_message::<ResetAttributes>()
            .add_message::<RaiseSkill>()
            .add_message::<UndoSkillPoints>()
            .add_message::<CommitSkillUndo>()
            .init_resource::<SkillUndo>()
            .configure_sets(
                Update,
                PlayerRequests
                    .in_set(InventorySet)
                    .after(init_inventory)
                    .before(sync_equipment),
            )
            .add_systems(
                Update,
                (
                    spend_attributes,
                    reset_attributes,
                    raise_skills,
                    undo_skill_points,
                )
                    .chain()
                    .in_set(PlayerRequests)
                    .run_if(resource_exists::<PlayerProgress>),
            );
    }
}

/// Where the player's point-spending requests are applied; their senders run before it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerRequests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    Physique,
    Cunning,
    Spirit,
}

#[derive(Message, Debug, Clone, Copy)]
pub struct SpendAttribute(pub Attribute);

/// Refund every spent attribute point.
#[derive(Message, Debug, Clone, Copy)]
pub struct ResetAttributes;

/// `CharacterBio::HasModifierPointsInUse`.
pub fn attributes_spent(loadout: &Loadout) -> bool {
    loadout.allocated != Default::default()
}

#[derive(Message, Debug, Clone)]
pub struct RaiseSkill {
    pub tree: String,
    pub skill: String,
}

/// Undo the skill points spent in the pane of `tree` since the window opened.
#[derive(Message, Debug, Clone)]
pub struct UndoSkillPoints {
    pub tree: String,
}

#[derive(Message, Debug, Clone, Copy)]
pub struct CommitSkillUndo;

/// One pane's changes: (skill, levels: spent > 0, refunded < 0) in order of first change, and the refunds
/// made (`UISkillPane.refundCount`).
#[derive(Default, Debug, Clone)]
pub struct PaneUndo {
    pub tree: String,
    pub levels: Vec<(String, i32)>,
    pub refunds: u32,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct SkillUndo {
    pub panes: Vec<PaneUndo>,
}

impl SkillUndo {
    fn pane(&mut self, tree: &str) -> &mut PaneUndo {
        let i = match self
            .panes
            .iter()
            .position(|p| p.tree.eq_ignore_ascii_case(tree))
        {
            Some(i) => i,
            None => {
                self.panes.push(PaneUndo {
                    tree: tree.to_owned(),
                    ..default()
                });
                self.panes.len() - 1
            }
        };
        &mut self.panes[i]
    }

    /// The trees whose pane's undo button is enabled.
    pub fn pending(&self) -> Vec<String> {
        self.panes
            .iter()
            .filter(|p| !p.levels.is_empty())
            .map(|p| p.tree.clone())
            .collect()
    }

    pub fn record(&mut self, tree: &str, skill: &str, delta: i32) {
        let levels = &mut self.pane(tree).levels;
        match levels
            .iter_mut()
            .find(|(s, _)| s.eq_ignore_ascii_case(skill))
        {
            Some(e) => e.1 += delta,
            None => levels.push((skill.to_owned(), delta)),
        }
    }

    pub fn refund(&mut self, tree: &str) {
        self.pane(tree).refunds += 1;
    }
}

fn spend_attributes(
    mut commands: Commands,
    mut msgs: MessageReader<SpendAttribute>,
    mut pr: ResMut<PlayerProgress>,
    mut players: Query<(Entity, &mut Loadout), With<Player>>,
) {
    let Ok((player, mut loadout)) = players.single_mut() else {
        msgs.clear();
        return;
    };
    for SpendAttribute(which) in msgs.read() {
        if pr.progress.attribute_points <= 0 {
            continue;
        }
        pr.progress.attribute_points -= 1;
        match which {
            Attribute::Physique => loadout.allocated.physique += 1,
            Attribute::Cunning => loadout.allocated.cunning += 1,
            Attribute::Spirit => loadout.allocated.spirit += 1,
        }
        info!(
            "attribute point spent: {:?} ({} left)",
            loadout.allocated, pr.progress.attribute_points
        );
        commands.entity(player).insert(ProfileDirty);
    }
}

/// `CharacterBio::ResetAttributePoints`.
fn reset_attributes(
    mut commands: Commands,
    mut msgs: MessageReader<ResetAttributes>,
    mut pr: ResMut<PlayerProgress>,
    mut players: Query<(Entity, &mut Loadout), With<Player>>,
) {
    let Ok((player, mut loadout)) = players.single_mut() else {
        msgs.clear();
        return;
    };
    for ResetAttributes in msgs.read() {
        let a = std::mem::take(&mut loadout.allocated);
        let n = a.physique + a.cunning + a.spirit;
        if n == 0 {
            continue;
        }
        pr.progress.attribute_points += n as i32;
        info!(
            "attributes reset: {n} points back ({} unspent)",
            pr.progress.attribute_points
        );
        commands.entity(player).insert(ProfileDirty);
    }
}

fn raise_skills(
    mut commands: Commands,
    mut msgs: MessageReader<RaiseSkill>,
    data: Res<GameData>,
    mut undo: ResMut<SkillUndo>,
    mut progress: ResMut<PlayerProgress>,
    mut players: Query<(Entity, &mut Loadout), With<Player>>,
) {
    let Ok((player, mut loadout)) = players.single_mut() else {
        msgs.clear();
        return;
    };
    for RaiseSkill { tree, skill } in msgs.read() {
        if progress.progress.skill_points <= 0 {
            continue;
        }
        let level = progress.progress.level;
        let trees = data.skill_trees();
        let trees = &*trees;
        let Some(t) = trees.tree(tree) else {
            info!("cannot raise {skill}: {tree} is no class tree");
            continue;
        };
        let new = loadout.skills.level(skill) == 0;
        if let Err(why) = loadout.skills.try_raise(&data.db, trees, t, skill, level) {
            info!("cannot raise {skill}: {why}");
            continue;
        }
        // A newly learned toggle starts switched off (`Skill_BuffSelfToggled` constructor; only
        // `AutoActivateNow` sets it).
        if new
            && data
                .db
                .get(skill)
                .is_some_and(|r| gd_sim::skills::is_toggle(&r.class))
        {
            loadout.skills.inactive.insert(skill.to_ascii_lowercase());
        }
        progress.progress.skill_points -= 1;
        undo.record(tree, skill, 1);
        info!(
            "skill point spent: {skill} -> {} ({} left)",
            loadout.skills.level(skill),
            progress.progress.skill_points
        );
        commands.entity(player).insert(ProfileDirty);
    }
}

#[allow(clippy::too_many_arguments)]
fn undo_skill_points(
    mut commands: Commands,
    data: Res<GameData>,
    mut undos: MessageReader<UndoSkillPoints>,
    mut commits: MessageReader<CommitSkillUndo>,
    mut undo: ResMut<SkillUndo>,
    mut progress: ResMut<PlayerProgress>,
    // Option: created with the player; no refunds before it exists.
    mut reclaim: Option<ResMut<crate::reclamation::SkillReclamation>>,
    mut players: Query<(Entity, &mut Loadout), With<Player>>,
) {
    let committed = commits.read().count() > 0;
    if let Ok((player, mut loadout)) = players.single_mut() {
        for UndoSkillPoints { tree } in undos.read() {
            let Some(i) = undo
                .panes
                .iter()
                .position(|p| p.tree.eq_ignore_ascii_case(tree))
            else {
                continue;
            };
            let pane = undo.panes.swap_remove(i);
            for (skill, d) in pane.levels {
                if d > 0 {
                    loadout.skills.lower(&skill, d as u32);
                } else {
                    loadout.skills.restore(&skill, d.unsigned_abs());
                }
                progress.progress.skill_points += d;
            }
            if let Some(r) = reclaim.as_mut()
                && pane.refunds > 0
            {
                let n = pane.refunds.min(r.used);
                let back = gd_sim::reclamation::payback(&data.db, r.used, n);
                r.used -= n;
                crate::loot::add_gold(&mut progress.gold, back as i64);
                info!("reclamation: undo {n} refund(s), {back} iron bits back");
            }
            commands.entity(player).insert(ProfileDirty);
        }
    } else {
        undos.clear();
    }
    if committed {
        undo.panes.clear();
    }
}
