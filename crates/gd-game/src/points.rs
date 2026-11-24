//! Spending attribute and skill points. The UI sends [`SpendAttribute`] /
//! [`RaiseSkill`]; the rules (enough points, `SkillBook::try_raise`) and
//! their effects (the loadout, unspent points, the profile) live here.
//!
//! Skills window undo (`UISkillPane` in GrimDawn.exe: click callback
//! `UISkillPane::OnButtonClick`, undo = `UISkillPane::Undo`, `UISkillPane::OnHide`,
//! enable flag in `UISkillPane::Update`): every point spent in the window
//! is recorded per skill ([`SkillUndo`], the entry's `SkillUndoEntry.delta`; the window's
//! `UISkillPane.undoPending` "something to undo" flag = a non-empty record). The button is enabled
//! only while that flag is set; undo takes every recorded level back, returns the
//! points and empties the record (it is all-or-nothing, not a stack). Hiding the
//! window ([`CommitSkillUndo`]) clears the record, making the spending permanent.
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

/// Undo every skill point spent since the skills window opened.
#[derive(Message, Debug, Clone, Copy)]
pub struct UndoSkillPoints;

/// The skills window was hidden: the spent points stay.
#[derive(Message, Debug, Clone, Copy)]
pub struct CommitSkillUndo;

/// Changes made in the open skills window: (skill, levels: spent > 0,
/// refunded < 0) in order of first change, and the refunds made.
#[derive(Resource, Default, Debug, Clone)]
pub struct SkillUndo {
    pub levels: Vec<(String, i32)>,
    pub refunds: u32,
}

impl SkillUndo {
    /// Whether the window's undo button is enabled.
    pub fn available(&self) -> bool {
        !self.levels.is_empty()
    }

    /// Records `delta` levels of `skill`.
    pub fn record(&mut self, skill: &str, delta: i32) {
        match self
            .levels
            .iter_mut()
            .find(|(s, _)| s.eq_ignore_ascii_case(skill))
        {
            Some(e) => e.1 += delta,
            None => self.levels.push((skill.to_owned(), delta)),
        }
    }

    fn clear(&mut self) {
        self.levels.clear();
        self.refunds = 0;
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
        undo.record(skill, 1);
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
    reclaim: Option<ResMut<crate::reclamation::SkillReclamation>>,
    mut players: Query<(Entity, &mut Loadout), With<Player>>,
) {
    let undone = undos.read().count() > 0;
    let committed = commits.read().count() > 0;
    if undone && let Ok((player, mut loadout)) = players.single_mut() {
        for (skill, d) in undo.levels.drain(..) {
            if d > 0 {
                loadout.skills.lower(&skill, d as u32);
            } else {
                loadout.skills.restore(&skill, d.unsigned_abs());
            }
            progress.progress.skill_points += d;
        }
        if let Some(mut r) = reclaim
            && undo.refunds > 0
        {
            let n = undo.refunds.min(r.used);
            let back = gd_sim::reclamation::payback(&data.db, r.used, n);
            r.used -= n;
            crate::loot::add_gold(&mut progress.gold, back as i64);
            info!("reclamation: undo {n} refund(s), {back} iron bits back");
        }
        undo.refunds = 0;
        commands.entity(player).insert(ProfileDirty);
    } else if committed {
        undo.clear();
    }
}
