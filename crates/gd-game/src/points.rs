//! Spending attribute and skill points. The UI sends [`SpendAttribute`] /
//! [`RaiseSkill`]; the rules (enough points, `SkillBook::try_raise`) and
//! their effects (the loadout, unspent points, the profile) live here.
//!
//! Skills window undo (`UISkillPane` in GrimDawn.exe: click callback
//! `UISkillPane::OnButtonClick`, undo = `UISkillPane::Undo`, `UISkillPane::OnHide`,
//! enable flag in `UISkillPane::Update`): every point spent in the window is recorded per pane, i.e. per
//! mastery (class tree), and per skill ([`SkillUndo`], the entry's `SkillUndoEntry.delta`; the pane's
//! `UISkillPane.undoPending` "something to undo" flag = a non-empty record). A pane's button is enabled
//! only while its flag is set; its undo ([`UndoSkillPoints`] naming the tree) takes every level recorded in
//! that pane back, returns those points and empties that record alone (all-or-nothing, not a stack): the
//! other pane keeps its spending. `UISkillPane::Undo` only walks its own skill list and re-checks no
//! requirement: each skill gets its own delta back. Nothing can be left orphaned, because a skill only
//! requires its own tree's mastery bar and every level that bar gained in this pane is reverted with it.
//! Hiding the window ([`CommitSkillUndo`]) clears every record, making the spending permanent.
//! Only skill points: attributes and devotion have their own buttons.
//! In reclamation mode (`crate::reclamation`) a click refunds a point instead;
//! the record keeps it as a negative entry and counts the refunds (the window's
//! `UISkillPane.refundCount`): undo levels those skills back up, takes the points again and pays
//! the price back (`UseReclamationPoints(-refunds)`,
//! `gd_sim::reclamation::payback`). The `StopSkill` call when a level falls
//! to 0 (a no-op on the base `Skill`) is not mirrored beyond the level.
//!
//! Attribute reset (Tonic of Reshaping, [`ResetAttributes`] from `consumables`):
//! `ItemAttributeReset::Use` → `ControllerCharacter::ResetAttributePoints` → the
//! `ResetAttributePointsConfigCmd::Execute` → `CharacterBio::ResetAttributePoints`: each
//! attribute's base goes back to its initial value, the points spent ((current − initial) / increment, summed)
//! are added to the unspent attribute points. Refused unless some point is spent (`HasModifierPointsInUse`).
//! The points spent are [`Loadout::allocated`], the save's included: the load splits them off the saved bio
//! (`gd_game_core::player::split_bio`), so earlier sessions' points are refunded too.
use crate::inventory::{init_inventory, sync_equipment};
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::inventory::InventorySet;
use gd_game_core::player::{Loadout, Player, ProfileDirty};
use gd_game_core::progress::PlayerProgress;
use gd_game_core::session::MessageDrain;

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
            )
            .drain_messages_unless::<SpendAttribute, _>(resource_exists::<PlayerProgress>)
            .drain_messages_unless::<ResetAttributes, _>(resource_exists::<PlayerProgress>)
            .drain_messages_unless::<RaiseSkill, _>(resource_exists::<PlayerProgress>)
            .drain_messages_unless::<UndoSkillPoints, _>(resource_exists::<PlayerProgress>)
            .drain_messages_unless::<CommitSkillUndo, _>(resource_exists::<PlayerProgress>);
    }
}

/// Where the player's requests (point spending) are applied; the game's
/// input systems that send them run before it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerRequests;

/// Physique, Cunning or Spirit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    Physique,
    Cunning,
    Spirit,
}

/// Spend one attribute point.
#[derive(Message, Debug, Clone, Copy)]
pub struct SpendAttribute(pub Attribute);

/// Refund every spent attribute point (a Tonic of Reshaping used, module doc).
#[derive(Message, Debug, Clone, Copy)]
pub struct ResetAttributes;

/// `CharacterBio::HasModifierPointsInUse`: some physique, cunning or spirit above its
/// initial value.
pub fn attributes_spent(loadout: &Loadout) -> bool {
    loadout.allocated != Default::default()
}

/// Spend one skill point on `skill` of class tree `tree`.
#[derive(Message, Debug, Clone)]
pub struct RaiseSkill {
    pub tree: String,
    pub skill: String,
}

/// Undo the skill points spent in the skills window's pane of class tree `tree` since the window opened.
#[derive(Message, Debug, Clone)]
pub struct UndoSkillPoints {
    pub tree: String,
}

/// The skills window was hidden: the spent points stay.
#[derive(Message, Debug, Clone, Copy)]
pub struct CommitSkillUndo;

/// One pane's changes in the open skills window: (skill, levels: spent > 0, refunded < 0) in order of first
/// change, and the refunds made (`UISkillPane.refundCount`).
#[derive(Default, Debug, Clone)]
pub struct PaneUndo {
    pub tree: String,
    pub levels: Vec<(String, i32)>,
    pub refunds: u32,
}

/// The open skills window's changes, per pane (module doc).
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

    /// Records `delta` levels of `skill` in the pane of `tree`.
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

    /// Counts one refund made in the pane of `tree`.
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

/// `CharacterBio::ResetAttributePoints` (module doc): the allocated points back to unspent.
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
        // A newly learned toggle starts switched off
        // (`Skill_BuffSelfToggled` constructor clears its active flag;
        // only `AutoActivateNow` sets it).
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
    // Option: created with the player (`reclamation::init_reclamation`); no refunds before it exists.
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
