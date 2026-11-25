//! Devotion: points, devotion shrines and celestial powers (rules in
//! `gd_sim::devotion`; the game shows the window and the shrines).
//!
//! * Points: the bio's `devotion_points` (unspent) / `total_devotion`; the
//!   story's `GiveDevotion` and restored shrines grant them, capped by
//!   `maxDevotionPoints` (see `gd_sim::devotion`).
//! * Shrines (`StaticShrine`): a shrine whose `<difficulty>Disabled` is set
//!   does not exist on that difficulty (`StaticShrine::Load`
//!   skips it). Using one already restored only says so
//!   (`RequestToUse`: `Player::IsShrineUIDKnown`);
//!   otherwise using it opens the shrine window ([`ShrineWindow`], see
//!   "Shrine window" below). A shrine with a `<difficulty>MonsterSpawn`
//!   proxy starts its fight when the window is accepted and is restored
//!   when every monster of it is dead (`RunProxy` / `AllKilled`); one with
//!   offerings (`<difficulty>Offering1..3`, one item each, read for the
//!   current difficulty only, `StaticShrine::Load`) takes them.
//!   Restoring gives `devotionPoints`, `<difficulty>XPReward` and the
//!   `<difficulty>LootTable` container's drops (`HandleShrineReward`),
//!   adds the uid to the restored and discovered lists (save block 17:
//!   per difficulty [restored, discovered]) and switches to the restored
//!   animation and looping effect.
//! * Celestial powers: a taken power (a star skill whose profile record, a
//!   wrapper's `buffSkillName`, has `templateAutoCast`)
//!   is bound to a skill (save: the bound skill's `autocast_skill` /
//!   `autocast_controller`) and gains the experience the player receives.
//!
//! A shrine counts as discovered within 50 m of the player
//! (`StaticShrine::UpdateSelf` → `Player::AddDiscoveredShrineUID`; not
//! `bindRadius`); a `<difficulty>Locked` shrine (`StaticShrine::Load`) stays
//! locked until a quest script calls `shrine:SetLocked(false)`
//! ([`story_devotion`]); the XP reward goes through
//! `Character::ReceiveExperience` (Bonus source 2, as quest XP: story
//! `GiveExperience` here, where the character's experience bonus % belongs);
//! the loot is thrown one item at a time from the shrine's drop point
//! (`StaticShrine::GetLootDropCoords` / `GetLootDropRadius`,
//! `crate::loot::fixed_drop_point`); bound powers gain the same experience the character receives
//! after its bonus % (`ReceiveExperience` → `SkillManager::AddExperience`)
//! from every source but `ExperienceSource` 1, which kills (the monster's
//! bonus), quests and shrines (2), notes and level gifts (0) do not use.
//!
//! Shrine window (Game.dll / `Grim Dawn.exe`):
//! * Using an unrestored shrine sends `EngageShrineRequest` (Game.dll
//!   `EngageShrineRequest::Execute`): a locked shrine answers `tagShrineLocked`, one restored
//!   for this player `tagShrineHostActivated`, one in use by another player
//!   `tagShrineBusy`; otherwise `EngageShrineResponse` (`EngageShrineResponse::Execute`) posts
//!   game event 60 with the shrine and `HasProxy`, and the game opens one
//!   of two windows from `records/ui/shrine/shrine_table.dbr` (`hudShrineWindow`):
//!   the "Ruined Shrine" offering window (loader `ShrineWindow::Load`) or, for a
//!   shrine with a proxy, the "Desecrated Shrine" window (loader
//!   `ShrineProxyWindow::Load`: `tagShrineConfirmProxy`, button `tagShrineButton02`),
//!   whose button sends `StartShrineProxyToServer` → `RunProxy`
//!   (`ShrineProxyWindow::OnButtonPress`).
//! * The offering window's update (`ShrineWindow::Update`) counts each offering with
//!   `Player::GetItemCountInStashes` (bags, then the material storage,
//!   private stash and transfer stash), less the earlier slots asking for
//!   the same item; a slot is filled when one is left ([`offering_slots`]).
//!   Its "Offer" button is enabled only when every slot is filled; its
//!   press (`ShrineWindow::OnOffer`) sends `CleanseShrineConfigCmd`, whose `Execute`
//!   takes one of each offering in the same order (`TakeOfferings`:
//!   `TakeItemFromCharacter`, `TakeItemFromReagents`, private stash,
//!   transfer), then `StaticShrine::CleanseShrine` rewards the players
//!   within `bindRadius` of it and switches it to restored.
//! * Cancel and the close button only hide the window; it also closes
//!   when the player is farther than the shrine's `GetUICloseDistance`
//!   (`uiCloseDistance`, 3D, `ShrineWindow::Update`). Nothing is taken until "Offer".
//! * Saving (the window's show handler `ShrineWindow::OnShow`): showing the
//!   offering window calls `GameEngine::SetSaveEnabled(false)`; hiding it (only
//!   when it was shown) calls `SetSaveEnabled(true)`, `AutoSave`,
//!   `SaveTransferStash` and `SaveReagents`. Here [`save_disabled`] holds the
//!   timed autosave back while the offering window is open and
//!   [`shrine_autosave`] sends a `SaveRequest` when it closes (the port writes
//!   the stash and reagents with every save). PORT: the proxy window's show
//!   handler was not decompiled, so it neither blocks nor triggers saves; F5,
//!   the menu's save and the quit save are the port's own and are not blocked.
//!   The `bindRadius` check
//!   is left out: a single player with the window open is always within
//!   it (`uiCloseDistance` 7 m vs `bindRadius` 80 m on stock shrines).
use crate::movement::MovePath;
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::map::{Uid, uid_bytes, uid_words};
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player, ProfileDirty};
use gd_rules::Rng;
use gd_sim::devotion::Devotions;
use gd_sim::items::ItemDb;

/// A celestial power bound to a skill.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    /// The skill that triggers it (`DEFAULT_ATTACK` for the plain attack).
    pub skill: String,
    pub power: String,
    pub controller: String,
    /// The item skill instance bound (`player::ItemSkillKey` with `skill`); none for a learned skill or the plain
    /// attack. The engine binds a `Skill` object, so two item skills of one record keep their own powers.
    pub item: Option<gd_game_core::player::ItemSkillSource>,
}

#[derive(Resource)]
pub struct DevotionState {
    pub difficulty: usize,
    pub points: u32,
    pub total: u32,
    pub restored: Vec<Uid>,
    pub discovered: Vec<Uid>,
    /// Monster totems cleared this session: not saved
    /// (`MonsterShrine::RequestToUse` / `UpdateSelf` keep only the entity's
    /// own state; unlike `StaticShrine::BindToLocalPlayer` they add no
    /// player shrine uid).
    pub totems_done: Vec<Uid>,
    /// Devotion points refunded so far (save block 8
    /// `devotion_reclamation_points_used`; prices the next refund).
    pub reclaimed: u32,
    /// Star skill (lower-case) → (devotion level, experience).
    pub powers: HashMap<String, (u32, u32)>,
    pub bindings: Vec<Binding>,
    pub data: Devotions,
    /// Bumped on every change (UI, autocasts); the window's view is the
    /// game's (`ui::devotion::DevotionView`).
    pub version: u64,
}

impl DevotionState {
    /// Grants points (capped at the rules' `max_devotion_points`); returns how many were added.
    pub fn grant(&mut self, n: u32, rules: &gd_game_core::rules::CombatRules) -> u32 {
        let max = rules.0.levels.max_devotion_points.max(0) as u32;
        let add = gd_sim::devotion::grant(self.total, max, n);
        self.points += add;
        self.total += add;
        self.version += 1;
        add
    }

    /// A celestial power's devotion level (`Skill::GetDevotionLevel`,
    /// `Skill.devotionLevel`: 0 until `SkillManager::AddExperience` raises it; 0 for any
    /// other skill), which its buffs carry (`crate::skill_buffs`).
    pub fn devotion_level(&self, skill: &str) -> u32 {
        self.powers
            .get(&skill.to_ascii_lowercase())
            .map_or(0, |p| p.0)
    }

    pub fn power_level(&self, skill: &str) -> u32 {
        self.powers
            .get(&skill.to_ascii_lowercase())
            .map_or(1, |p| p.0.max(1))
    }

    /// Binds `power` to `skill` (`item`: that item skill instance; a power drives one skill, a skill one power).
    pub fn bind(
        &mut self,
        db: &gd_formats::vfs::Database,
        power: &str,
        skill: &str,
        item: Option<gd_game_core::player::ItemSkillSource>,
    ) -> bool {
        // `templateAutoCast` of the power's profile record (a wrapper's
        // `buffSkillName`: `Skill::LoadResources` reads it).
        let Some(controller) = gd_sim::skills::profile_record(db, power)
            .and_then(|r| r.str("templateAutoCast").map(str::to_owned))
            .filter(|c| !c.is_empty())
        else {
            return false;
        };
        self.bindings.retain(|b| {
            !b.power.eq_ignore_ascii_case(power)
                && !(b.skill.eq_ignore_ascii_case(skill) && b.item == item)
        });
        self.bindings.push(Binding {
            skill: skill.to_owned(),
            power: power.to_owned(),
            controller,
            item,
        });
        self.version += 1;
        true
    }

    pub fn unbind(&mut self, star: &str) {
        self.bindings
            .retain(|b| !b.power.eq_ignore_ascii_case(star));
        self.version += 1;
    }
}

pub const DIFFICULTY: [&str; 3] = ["normal", "epic", "legendary"];

pub struct DevotionRequestsPlugin;

impl Plugin for DevotionRequestsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TakeStar>()
            .add_message::<RemoveStar>()
            .add_message::<BindPower>()
            .add_message::<UndoDevotion>()
            .add_message::<CommitDevotionUndo>()
            .init_resource::<DevotionUndo>()
            .add_systems(
                Update,
                (devotion_requests, undo_devotion)
                    .chain()
                    .in_set(crate::points::PlayerRequests)
                    .run_if(resource_exists::<DevotionState>),
            );
    }
}

/// Spend a devotion point on star `star` of constellation `constellation` (refused in the reclaim
/// mode, [`devotion_requests`]).
#[derive(Message, Clone, Copy, Debug)]
pub struct TakeStar {
    pub constellation: usize,
    pub star: usize,
}

/// Refund star `star` of constellation `constellation` (costs gold and
/// aether, [`reclaim_cost`]); only in the devotion window's reclaim mode
/// ([`devotion_requests`]).
#[derive(Message, Clone, Copy, Debug)]
pub struct RemoveStar {
    pub constellation: usize,
    pub star: usize,
}

/// Bind the taken celestial power `power` to the skill `skill` (`None`: unbind it).
#[derive(Message, Clone, Debug)]
pub struct BindPower {
    pub power: String,
    pub skill: Option<std::sync::Arc<str>>,
    /// The item skill instance picked (the skill-select list's row); none for a learned skill, and for a hot slot
    /// key, whose slot names a skill only: then an unlearned skill's first worn, else cached, instance.
    pub item: Option<gd_game_core::player::ItemSkillSource>,
}

/// Undo every star taken or refunded since the devotion window opened.
#[derive(Message, Clone, Copy, Debug)]
pub struct UndoDevotion;

/// The devotion window was hidden: the changes stay.
#[derive(Message, Clone, Copy, Debug)]
pub struct CommitDevotionUndo;

/// One change made in the open devotion window.
#[derive(Clone, Debug)]
pub enum DevotionStep {
    Taken(String),
    /// A refunded star with what the refund cost (gold, aether) and the
    /// power's level / experience and bindings it dropped.
    Removed {
        skill: String,
        cost: (u64, u32),
        power: Option<(u32, u32)>,
        bindings: Vec<Binding>,
    },
}

/// Changes made in the open devotion window, in order.
///
/// Devotion pane (GrimDawn.exe): the undo button is the pane's
/// `undoButton` (click callback `DevotionWindow::OnButtonEvent`, sender `DevotionWindow.undoButton`). Each star
/// counts the clicks made in the window (`DevotionStar.clickCount`; the pane's "something
/// to undo" flag `DevotionWindow.canUndo` is set by any, `DevotionWindow::HandleStarClick`, and enables the button:
/// `DevotionWindow::Update` disables it while clear); the window's total reclamation delta
/// is `DevotionWindow.reclaimDelta`. Undo walks every star with a count: when the pane is in
/// reclaim mode (`DevotionWindow.reclaimMode`, set while removing stars) it levels the skill up
/// again and spends the points (`SubtractDevotionPoint`), otherwise it takes the
/// skill away (`Skill::SetSkillLevel(0)`) and returns the points (`AddDevotionPoints`);
/// it then calls `SkillManager::UseDevotionReclamationPoints(-removals)`, which
/// lowers the refund count and gives back the gold and aether the removals cost
/// (Game.dll `SkillManager::RefundDevotionReclamation`: negative count, cost of the tier at each lowered
/// count), clears the counts, `canUndo` and `reclaimDelta`. It is all-or-nothing.
/// Hiding the pane (`DevotionWindow::SetVisible` with 0: leaves reclaim mode; the tab switch
/// asks `tagConfirmSkillChanges`, `DevotionWindow::ConfirmTabSwitch`, then `DevotionWindow::ClearPendingChanges`
/// resets `canUndo` / `reclaimDelta`) makes the changes final.
/// PORT: the engine has one pane-wide mode and unordered counts; here every
/// click is a step undone newest first (same result for one mode). A refunded
/// power's experience and bindings are restored (the engine keeps them on the
/// skill), aether crystals return to the material storage; while that store is blocked (`crate::stash`
/// module doc, `SharedFiles::accepts`) they go into the bags, and with no room there the undo is refused
/// (error sound, nothing changes).
#[derive(Resource, Default, Debug, Clone)]
pub struct DevotionUndo(pub Vec<DevotionStep>);

impl DevotionUndo {
    /// Whether the window's undo button is enabled.
    pub fn available(&self) -> bool {
        !self.0.is_empty()
    }
}

/// Gold and aether for refunding one devotion point after `reclaimed`
/// refunds (`GetCurrentDevotionReclamationCost`: the cost of the first tier
/// band `[tiers[i], tiers[i + 1])` holding the count, else the last).
pub fn reclaim_cost(db: &gd_formats::vfs::Database, reclaimed: u32) -> (u64, u32) {
    let Some(rec) = db.get(gd_sim::reclamation::PLAYER_RECORD) else {
        return (0, 0);
    };
    let tiers = rec.f32s("devotionReclamationPointTiers");
    let costs = rec.f32s("devotionReclamationPointCosts");
    let c = reclaimed as f32;
    let i = tiers
        .windows(2)
        .position(|w| c >= w[0] && c < w[1])
        .unwrap_or(tiers.len().saturating_sub(1));
    let gold = costs.get(i).copied().unwrap_or(0.0).max(0.0) as u64;
    (
        gold,
        rec.i32("devotionReclamationAetherCost").unwrap_or(0).max(0) as u32,
    )
}

/// The aether crystals the player holds in the bags, the stashes and the material storage
/// (`Player::GetCurrentAether`, what a refund takes; the crystal is `gameengine.dbr` `aetherCrystal`).
pub fn aether_held(
    data: &GameData,
    inv: &gd_game_core::inventory::Inventory,
    stash: &gd_game_core::stash::Stash,
) -> u32 {
    gd_game_core::stash::count(data.engine().aether_crystal.as_slice(), inv, stash)
}

/// Applies the devotion window's requests: taking a star (a point, the
/// constellation rules `Devotions::cannot_take`), refunding one (gold and
/// aether crystals, `cannot_remove`), binding a power.
///
/// The window has two modes (GrimDawn.exe star click `DevotionWindow::OnStarClick`, pane flag
/// `DevotionWindow.reclaimMode`): opened plainly, a left click takes a star and nothing refunds one (no other
/// button reaches the stars, `DevotionWindow::HandleStarClick`); opened by a spirit guide (`crate::reclamation`,
/// `SkillReclamation::devotion`) a left
/// click on a taken star refunds it (`SkillManager::UseDevotionReclamationPoints(1)`, then the point back,
/// `AddDevotionPoints`, a celestial power's binding dropped) and nothing takes one. So a [`TakeStar`] is
/// refused in the mode and a [`RemoveStar`] outside it.
#[allow(clippy::too_many_arguments)]
pub fn devotion_requests(
    mut commands: Commands,
    mut take: MessageReader<TakeStar>,
    mut remove: MessageReader<RemoveStar>,
    mut bind: MessageReader<BindPower>,
    mut state: ResMut<DevotionState>,
    mut players: Query<(Entity, &mut Loadout), With<Player>>,
    data: Res<GameData>,
    // Option: the refund pays from whichever of these exist.
    mut inv: Option<ResMut<gd_game_core::inventory::Inventory>>,
    mut stash: Option<ResMut<gd_game_core::stash::Stash>>,
    mut progress: Option<ResMut<gd_game_core::progress::PlayerProgress>>,
    mut undo: ResMut<DevotionUndo>,
    // Option: created with the player (`reclamation::init_reclamation`); without it nothing refunds.
    reclaim: Option<Res<crate::reclamation::SkillReclamation>>,
) {
    let reclaiming = reclaim.is_some_and(|r| r.devotion_mode());
    let Ok((player, mut loadout)) = players.single_mut() else {
        take.clear();
        remove.clear();
        bind.clear();
        return;
    };
    for b in bind.read() {
        match &b.skill {
            Some(skill) => {
                let item = b.item.clone().or_else(|| {
                    (&**skill != gd_game_core::casting::DEFAULT_ATTACK
                        && loadout.skills.level(skill) == 0)
                        .then(|| loadout.item_skill_source(skill))
                        .flatten()
                });
                if loadout.skills.level(&b.power) > 0 && state.bind(&data.db, &b.power, skill, item)
                {
                    info!("devotion: {} bound to {skill}", b.power);
                }
            }
            None => state.unbind(&b.power),
        }
    }
    for t in take.read() {
        let Some(star) = state
            .data
            .list
            .get(t.constellation)
            .and_then(|c| c.stars.get(t.star))
        else {
            continue;
        };
        let skill = star.skill.clone();
        if reclaiming {
            info!("devotion: cannot take {skill} in the reclaim mode");
            continue;
        }
        let taken = |n: &str| loadout.skills.level(n) > 0;
        if let Some(why) = state
            .data
            .cannot_take(t.constellation, t.star, &taken, state.points)
        {
            info!("devotion: cannot take {skill}: {why}");
            continue;
        }
        loadout.skills.skills.push((skill.clone(), 1));
        state.points -= 1;
        undo.0.push(DevotionStep::Taken(skill.clone()));
        state.powers.entry(skill.clone()).or_insert((1, 0));
        state.version += 1;
        info!("devotion: took {skill} ({} points left)", state.points);
        commands.entity(player).insert(ProfileDirty);
    }
    for r in remove.read() {
        let Some(star) = state
            .data
            .list
            .get(r.constellation)
            .and_then(|c| c.stars.get(r.star))
        else {
            continue;
        };
        let skill = star.skill.clone();
        if !reclaiming {
            info!("devotion: cannot remove {skill} outside the reclaim mode (a spirit guide)");
            continue;
        }
        let taken = |n: &str| loadout.skills.level(n) > 0;
        if let Some(why) = state.data.cannot_remove(r.constellation, r.star, &taken) {
            info!("devotion: cannot remove {skill}: {why}");
            continue;
        }
        let (Some(inv), Some(stash), Some(progress)) = (
            inv.as_deref_mut(),
            stash.as_deref_mut(),
            progress.as_deref_mut(),
        ) else {
            continue;
        };
        let (gold, aether) = reclaim_cost(&data.db, state.reclaimed);
        if progress.gold < gold || (aether > 0 && aether_held(&data, inv, stash) < aether) {
            info!("devotion: cannot remove {skill}: refund costs {gold} gold and {aether} aether");
            continue;
        }
        progress.gold -= gold;
        if aether > 0 {
            gd_game_core::stash::take(data.engine().aether_crystal.as_slice(), aether, inv, stash);
        }
        state.reclaimed += 1;
        loadout
            .skills
            .skills
            .retain(|(n, _)| !n.eq_ignore_ascii_case(&skill));
        state.points += 1;
        let bindings = state
            .bindings
            .iter()
            .filter(|b| b.power.eq_ignore_ascii_case(&skill))
            .cloned()
            .collect();
        undo.0.push(DevotionStep::Removed {
            skill: skill.clone(),
            cost: (gold, aether),
            power: state.powers.remove(&skill),
            bindings,
        });
        state.unbind(&skill);
        info!("devotion: removed {skill} ({} points left)", state.points);
        commands.entity(player).insert(ProfileDirty);
    }
}

/// The window's undo (see [`DevotionUndo`]) or its close.
#[allow(clippy::too_many_arguments)]
pub fn undo_devotion(
    mut commands: Commands,
    mut undos: MessageReader<UndoDevotion>,
    mut commits: MessageReader<CommitDevotionUndo>,
    mut undo: ResMut<DevotionUndo>,
    mut state: ResMut<DevotionState>,
    mut players: Query<(Entity, &mut Loadout), With<Player>>,
    // Option: the refunds go back to whichever of these exist.
    (mut stash, mut inv): (
        Option<ResMut<gd_game_core::stash::Stash>>,
        Option<ResMut<gd_game_core::inventory::Inventory>>,
    ),
    mut progress: Option<ResMut<gd_game_core::progress::PlayerProgress>>,
    data: Res<GameData>,
    (shared, choice, save_dir, mut error): (
        Res<gd_game_core::stash::SharedFiles>,
        Res<gd_game_core::session::Choice>,
        Res<gd_game_core::session::SaveDir>,
        MessageWriter<gd_game_core::stash::QuickActionError>,
    ),
) {
    let undone = undos.read().count() > 0;
    let committed = commits.read().count() > 0;
    if undone
        && !undo.0.is_empty()
        && let Ok((player, mut loadout)) = players.single_mut()
    {
        let engine = data.engine();
        let crystal = engine.aether_crystal.as_deref();
        // The material storage takes the crystals back only while the save writes it (`crate::stash` module
        // doc); else all of them go into the bags first, or the undo is refused.
        let mut to_storage = true;
        let aether: u32 = undo
            .0
            .iter()
            .map(|s| match s {
                DevotionStep::Removed {
                    cost: (_, aether), ..
                } => *aether,
                DevotionStep::Taken(_) => 0,
            })
            .sum();
        if aether > 0
            && let Some(c) = crystal
            && !shared.accepts(&choice, &save_dir, gd_formats::save::Shared::Reagents)
        {
            to_storage = false;
            let item = gd_formats::save::item::Item {
                base_name: c.to_owned(),
                seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
                stack_count: aether,
                ..Default::default()
            };
            if !inv.as_deref_mut().is_some_and(|i| i.add_to_bags(item)) {
                info!("devotion: no room in the bags for the refunded aether; undo refused");
                error.write(gd_game_core::stash::QuickActionError { notify: false });
                return;
            }
        }
        for step in undo.0.drain(..).rev() {
            match step {
                DevotionStep::Taken(skill) => {
                    loadout
                        .skills
                        .skills
                        .retain(|(n, _)| !n.eq_ignore_ascii_case(&skill));
                    state.powers.remove(&skill);
                    state.unbind(&skill);
                    state.points += 1;
                }
                DevotionStep::Removed {
                    skill,
                    cost: (gold, aether),
                    power,
                    bindings,
                } => {
                    loadout.skills.skills.push((skill.clone(), 1));
                    state.powers.insert(skill, power.unwrap_or((1, 0)));
                    state.bindings.extend(bindings);
                    state.points = state.points.saturating_sub(1);
                    state.reclaimed = state.reclaimed.saturating_sub(1);
                    if let Some(p) = progress.as_deref_mut() {
                        crate::loot::add_gold(&mut p.gold, gold as i64);
                    }
                    if aether > 0
                        && to_storage
                        && let (Some(st), Some(c)) = (stash.as_deref_mut(), crystal)
                    {
                        gd_game_core::stash::store_material(&mut st.materials, c, aether);
                    }
                }
            }
        }
        state.version += 1;
        commands.entity(player).insert(ProfileDirty);
    } else if committed {
        undo.0.clear();
    }
}

/// What the shrine window's requests need: the devotion state and the bags (the offerings).
fn shrine_window_inputs(
    state: Option<Res<DevotionState>>,
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
) -> bool {
    state.is_some() && inv.is_some()
}

pub struct DevotionPlugin;

impl Plugin for DevotionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShrineWindow>()
            .add_message::<ShrineLock>()
            .add_message::<ShrineRequest>()
            .add_message::<ShrineCleansed>()
            .add_systems(
                Update,
                (
                    init_devotion.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    story_devotion.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    power_experience
                        .run_if(resource_exists::<gd_game_core::progress::PlayerProgress>)
                        .run_if(resource_exists::<DevotionState>),
                    discover_shrines.run_if(resource_exists::<DevotionState>),
                    use_shrine.run_if(resource_exists::<DevotionState>),
                    shrine_requests.run_if(shrine_window_inputs),
                    shrine_autosave,
                    shrine_fights.run_if(resource_exists::<DevotionState>),
                )
                    .chain()
                    .after(gd_game_core::player::PlayerSet),
            );
    }
}

/// The open shrine window: the shrine and whether it is the proxy
/// ("Desecrated Shrine", Start) window rather than the offering one.
/// `version` changes whenever it opens or closes.
#[derive(Resource, Default)]
pub struct ShrineWindow {
    pub shrine: Option<Entity>,
    pub proxy: bool,
    pub version: u64,
}

impl ShrineWindow {
    fn set(&mut self, shrine: Option<Entity>, proxy: bool) {
        self.shrine = shrine;
        self.proxy = proxy;
        self.version += 1;
    }
}

/// Whether the offering window is open, which disables saving
/// (`SetSaveEnabled(false)`, see the module doc).
pub fn save_disabled(window: &ShrineWindow) -> bool {
    window.shrine.is_some() && !window.proxy
}

/// Closing the offering window autosaves (`GameEngine::AutoSave`).
pub fn shrine_autosave(
    window: Res<ShrineWindow>,
    mut was_open: Local<bool>,
    mut fresh: gd_game_core::session::NewSession,
    mut save: MessageWriter<crate::save::SaveRequest>,
) {
    // The window of a finished session is gone with it: no close to autosave.
    if fresh.begun() {
        *was_open = false;
    }
    let open = save_disabled(&window);
    if *was_open && !open {
        info!("devotion: shrine window closed, autosave");
        save.write(crate::save::SaveRequest);
    }
    *was_open = open;
}

/// The player's answer to the shrine window.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShrineRequest {
    /// "Offer" (takes the offerings and restores the shrine; ignored unless
    /// every slot is filled) or, for a proxy shrine, "Start" (its fight).
    Accept,
    /// Cancel, the close button, Esc.
    Cancel,
}

/// `CleanseShrine`: the offerings were taken; restore the shrine and pay
/// its reward ([`shrine_fights`], which also does it when a fight ends).
#[derive(Message, Clone, Copy, Debug)]
pub struct ShrineCleansed(pub Entity);

/// The window's three offering slots (`shrineBox1..3`): the offering of
/// each (None: no offering, the slot is covered) and whether the player has
/// it, counting each item once per slot asking for it (`ShrineWindow::Update`).
pub fn offering_slots(
    sh: &Shrine,
    inv: &gd_game_core::inventory::Inventory,
    stash: Option<&gd_game_core::stash::Stash>,
) -> [Option<(String, bool)>; 3] {
    let count = |rec: &String| {
        stash.map_or_else(
            || inv.count_by_record(rec),
            |s| gd_game_core::stash::count(std::slice::from_ref(rec), inv, s),
        )
    };
    std::array::from_fn(|i| {
        let rec = sh.offerings[i].as_ref()?;
        let earlier = sh.offerings[..i]
            .iter()
            .flatten()
            .filter(|o| o.eq_ignore_ascii_case(rec))
            .count() as u32;
        let have = count(rec) > earlier;
        Some((rec.clone(), have))
    })
}

/// `StaticShrine::UpdateSelf`: discovered within 50 m.
const DISCOVER_RANGE: f32 = 50.0;

/// A script's `shrine:SetLocked` on the shrine of `record` at `at` (the game
/// resolves the story object).
#[derive(Message, Clone, Debug)]
pub struct ShrineLock {
    pub record: String,
    pub at: Vec3,
    pub locked: bool,
}

/// A devotion shrine placed in a streamed-in level.
#[derive(Component)]
pub struct Shrine {
    pub uid: Uid,
    pub record: String,
    /// `Actor::GetRadius` (`combat::get_radius`): the use range's and the hover pick's.
    pub radius: f32,
    /// The record's `usableRange` (`combat::fixed_usable_range`).
    pub range: f32,
    pub points: u32,
    /// `<difficulty>Offering1..3` (the window's slots 1–3).
    pub offerings: [Option<String>; 3],
    pub proxy: Option<String>,
    pub xp: u32,
    pub loot: Option<String>,
    pub locked: Option<String>,
    /// A monster totem (`MonsterShrine`): a fight for XP and loot, no
    /// devotion point, not kept in the save's shrine lists.
    pub totem: bool,
    /// A running fight: the proxy's monsters.
    pub fight: Option<Vec<Entity>>,
}

impl Shrine {
    /// `IsOfInterest`: `StaticShrine` while dormant
    /// (state 1), 4, or restored (6) and not `<difficulty>Disabled` (a disabled
    /// one is not spawned here), so not during its fight (2) or the restoring
    /// animation (5); `MonsterShrine` only while dormant (state 1:
    /// not fighting, not cleared).
    pub fn is_of_interest(&self, state: Option<&DevotionState>) -> bool {
        self.fight.is_none()
            && !(self.totem && state.is_some_and(|s| s.totems_done.contains(&self.uid)))
    }
}

/// Spawns a shrine (None when disabled on this difficulty); the game
/// dresses it.
pub fn spawn_shrine(
    commands: &mut Commands,
    db: &gd_formats::vfs::Database,
    tf: Transform,
    record: &str,
    uid: Uid,
    mode: gd_rules::mode::Mode,
) -> Option<Entity> {
    let rec = db.get(record)?;
    let difficulty = mode.difficulty;
    let d = DIFFICULTY[difficulty.min(2)];
    let f = |k: &str| {
        rec.str(&format!("{d}{k}"))
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    if rec.i32(&format!("{d}Disabled")).unwrap_or(0) != 0 {
        return None;
    }
    let shrine = Shrine {
        uid,
        record: record.to_owned(),
        radius: gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS),
        range: gd_game_core::combat::fixed_usable_range(Some(rec)),
        points: rec.i32("devotionPoints").unwrap_or(0).max(0) as u32,
        offerings: std::array::from_fn(|i| f(&format!("Offering{}", i + 1))),
        proxy: f("MonsterSpawn"),
        xp: rec.i32(&format!("{d}XPReward")).unwrap_or(0).max(0) as u32,
        // Ultimate Veteran: `ascendantLootTable` (`StaticShrine::Load`).
        loot: if mode.ultimate_veteran() {
            rec.str("ascendantLootTable")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| f("LootTable"))
        } else {
            f("LootTable")
        },
        locked: (rec.i32(&format!("{d}Locked")).unwrap_or(0) != 0)
            .then(|| rec.str("lockedTextTag").unwrap_or("").to_owned()),
        totem: rec.class == "MonsterShrine",
        fight: None,
    };
    debug!("devotion: shrine {record} at {:?}", tf.translation);
    let interact = crate::player_orders::Interactable::fixed(shrine.radius);
    Some(
        commands
            .spawn((
                tf,
                GlobalTransform::from(tf),
                shrine,
                interact,
                Name::new(format!(
                    "shrine {}",
                    record.rsplit('/').next().unwrap_or("")
                )),
            ))
            .id(),
    )
}

pub fn init_devotion(
    mut commands: Commands,
    // Option: `have` is the existence check (this system creates it); `loaded` is absent for a new character.
    have: Option<Res<DevotionState>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    data: Res<gd_game_core::data::GameData>,
    players: Query<(), With<Player>>,
) {
    if have.is_some() || players.is_empty() {
        return;
    }
    let mut s = DevotionState {
        difficulty: 0,
        points: 0,
        total: 0,
        restored: Vec::new(),
        discovered: Vec::new(),
        totems_done: Vec::new(),
        reclaimed: 0,
        powers: HashMap::default(),
        bindings: Vec::new(),
        data: Devotions::load(&data.db),
        version: 1,
    };
    if let Some(c) = &loaded {
        let g = &c.0;
        let d = g.info.difficulty();
        s.difficulty = d;
        s.points = g.bio.devotion_points;
        s.reclaimed = g.skills.devotion_reclamation_points_used;
        s.total = g.bio.total_devotion;
        s.restored = g.shrines.lists[d][0].iter().map(uid_words).collect();
        s.discovered = g.shrines.lists[d][1].iter().map(uid_words).collect();
        for k in &g.skills.skills {
            if s.data.locate(&k.name).is_some() {
                s.powers.insert(
                    k.name.to_ascii_lowercase(),
                    (k.devotion_level, k.devotion_experience),
                );
            }
            if !k.autocast_skill.is_empty() && !k.autocast_controller.is_empty() {
                s.bindings.push(Binding {
                    skill: k.name.clone(),
                    power: k.autocast_skill.clone(),
                    controller: k.autocast_controller.clone(),
                    item: None,
                });
            }
        }
        // An item skill's binding (`player::item_skill_binding_restored`).
        for k in &g.skills.item_skills {
            if k.autocast_skill.is_empty() || k.autocast_controller.is_empty() {
                continue;
            }
            if let Some(item) = gd_game_core::player::item_skill_binding_restored(&data.db, g, k) {
                s.bindings.push(Binding {
                    skill: k.skill_name.clone(),
                    power: k.autocast_skill.clone(),
                    controller: k.autocast_controller.clone(),
                    item: Some(item),
                });
            }
        }
    }
    info!(
        "devotion: {} constellations, {} / {} points, {} shrines restored",
        s.data.list.len(),
        s.points,
        s.total,
        s.restored.len()
    );
    commands.insert_resource(s);
}

/// Story grants, and the scripts' `shrine:SetLocked` (a shrine
/// starts locked from `<difficulty>Locked`, `StaticShrine::Load`, and quest
/// scripts unlock it, e.g. `homesteadquestoldarkovia.lua`). The script's
/// object is matched to the level's shrine by record and position; the
/// state is remembered for shrines streamed in later.
#[allow(clippy::too_many_arguments)]
pub fn story_devotion(
    mut fresh: gd_game_core::session::NewSession,
    mut events: MessageReader<gd_game_core::story::StoryEffect>,
    // Option: the story effects of other kinds are handled without devotion.
    mut state: Option<ResMut<DevotionState>>,
    mut shrine_locks: MessageReader<ShrineLock>,
    rules: Res<gd_game_core::rules::CombatRules>,
    data: Res<gd_game_core::data::GameData>,
    mut shrines: Query<(&Transform, &mut Shrine)>,
    mut locks: Local<Vec<(String, Vec3, bool)>>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
) {
    if fresh.begun() {
        *locks = Default::default();
    }
    for ev in events.read() {
        if let gd_story::story::Effect::GiveDevotion(n) = &ev.0
            && let Some(s) = state.as_mut()
        {
            let add = s.grant(*n, &rules);
            info!(
                "devotion: +{add} point(s) from the story ({} unspent)",
                s.points
            );
            // `ScriptableAction_GiveDevotion`: the points
            // granted (clamped by the cap) as the headline, type 4; nothing when none were.
            if add > 0 {
                let tag = if add == 1 {
                    "tagDevotionPointAwarded"
                } else {
                    "tagDevotionPointsAwarded"
                };
                let head = data
                    .tags()
                    .get(tag)
                    .unwrap_or(tag)
                    .replace("{%d0}", &add.to_string());
                cinematic.write(gd_game_core::story::CinematicText::plain(head, 4));
            }
        }
    }
    let new_locks = !shrine_locks.is_empty();
    for l in shrine_locks.read() {
        locks.retain(|(r, p, _)| !(r.eq_ignore_ascii_case(&l.record) && p.distance(l.at) < 1.0));
        locks.push((l.record.clone(), l.at, l.locked));
    }
    // The match only changes with a new lock or a shrine streamed in.
    if locks.is_empty() || (!new_locks && !shrines.iter_mut().any(|(_, sh)| sh.is_added())) {
        return;
    }
    for (tf, mut sh) in &mut shrines {
        let Some(&(_, _, locked)) = locks.iter().find(|(r, p, _)| {
            r.eq_ignore_ascii_case(&sh.record) && p.distance(tf.translation) < 1.0
        }) else {
            continue;
        };
        if sh.locked.is_some() != locked {
            sh.locked = locked.then(|| {
                data.db
                    .get(&sh.record)
                    .and_then(|r| r.str("lockedTextTag").map(str::to_owned))
                    .unwrap_or_default()
            });
            info!(
                "devotion: shrine {} {}",
                sh.record.rsplit('/').next().unwrap_or(""),
                if locked { "locked" } else { "unlocked" }
            );
        }
    }
}

/// Bound celestial powers gain the experience the player receives: the growth of the session's
/// `experience` since the last frame (`last` starts over with each session, so loading a character with
/// more experience than the previous one is not a gain).
pub fn power_experience(
    p: Res<gd_game_core::progress::PlayerProgress>,
    mut s: ResMut<DevotionState>,
    players: Query<&gd_game_core::player::Loadout, With<Player>>,
    data: Res<gd_game_core::data::GameData>,
    mut fresh: gd_game_core::session::NewSession,
    mut last: Local<Option<u64>>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
) {
    if fresh.begun() {
        *last = None;
    }
    let xp = p.progress.experience;
    let prev = last.replace(xp).unwrap_or(xp);
    if xp <= prev {
        return;
    }
    let gain = (xp - prev).min(u32::MAX as u64) as u32;
    let Ok(loadout) = players.single() else {
        return;
    };
    let bound: Vec<String> = s
        .bindings
        .iter()
        .filter(|b| {
            b.skill == gd_game_core::casting::DEFAULT_ATTACK || loadout.skills.level(&b.skill) > 0
        })
        .map(|b| b.power.to_ascii_lowercase())
        .collect();
    for power in bound {
        // The profile record's table (a wrapper's `buffSkillName`, e.g. Dying God).
        let Some(rec) = gd_sim::skills::profile_record(&data.db, &power) else {
            continue;
        };
        let levels: Vec<i32> = rec
            .f32s_iter("skillExperienceLevels")
            .map(|v| v as i32)
            .collect();
        let e = s.powers.entry(power.clone()).or_insert((1, 0));
        let before = e.0;
        *e = gd_sim::devotion::add_power_experience(&levels, e.0, e.1, gain);
        if e.0 != before {
            info!(
                "devotion: {} reached level {}",
                power.rsplit('/').next().unwrap_or(""),
                e.0
            );
            // `SkillManager::AddExperience`: type 4, `tagDevotionUpdate`
            // over `tagDevotionSkillLevelUp(skill name, rank)`, or `tagDevotionSkillMaxLevel(name)` at the last rank.
            let name = rec.str("skillDisplayName").unwrap_or_default().to_owned();
            let max = e.0 as usize >= levels.len();
            let mut args = vec![gd_game_core::floating::TextArg::Tag(name)];
            if !max {
                args.push(gd_game_core::floating::TextArg::Num(e.0 as f32));
            }
            cinematic.write(gd_game_core::story::CinematicText {
                head: "tagDevotionUpdate".into(),
                sub: if max {
                    "tagDevotionSkillMaxLevel"
                } else {
                    "tagDevotionSkillLevelUp"
                }
                .into(),
                sub_args: args,
                kind: 4,
                front: false,
            });
            s.version += 1;
        }
    }
}

pub fn discover_shrines(
    mut s: ResMut<DevotionState>,
    shrines: Query<(&GlobalTransform, &Shrine)>,
    players: Query<&Transform, With<Player>>,
) {
    let Ok(ptf) = players.single() else {
        return;
    };
    for (gt, sh) in &shrines {
        if !sh.totem
            && gt.translation().distance(ptf.translation) <= DISCOVER_RANGE
            && !s.discovered.contains(&sh.uid)
        {
            s.discovered.push(sh.uid);
            debug!("devotion: shrine discovered at {:?}", gt.translation());
        }
    }
}

/// Within reach of the clicked shrine: say why it can't be used, or open
/// its window (a monster totem starts its fight at once,
/// `MonsterShrine::RequestToUse` has no window).
#[allow(clippy::too_many_arguments)]
pub fn use_shrine(
    mut commands: Commands,
    #[cfg(feature = "dev")] mut scratch: Local<gd_sim::nav::PathScratch>,
    mut window: ResMut<ShrineWindow>,
    mut shrines: Query<(Entity, &GlobalTransform, &mut Shrine)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
            &gd_game_core::character::Character,
        ),
        With<Player>,
    >,
    s: Res<DevotionState>,
    mut notify: MessageWriter<gd_game_core::story::Notify>,
    mut fights: ShrineFights,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    #[cfg(feature = "dev")] nav: Res<crate::nav::Nav>,
    data: Res<gd_game_core::data::GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    #[cfg(feature = "dev")] mut tested: Local<bool>,
) {
    let Ok((ptf, mut path, mut interact, pcb, ch)) = players.single_mut() else {
        return;
    };
    // `GD_USE_SHRINE` (hooks.rs): use the nearest shrine within 40 m once (opens its window).
    #[cfg(feature = "dev")]
    if hooks.gameplay.use_shrine && !*tested && interact.0.is_none() {
        let near = shrines
            .iter()
            .map(|(e, gt, sh)| {
                (
                    e,
                    gt.translation(),
                    sh.radius,
                    gt.translation().distance(ptf.translation),
                )
            })
            .filter(|x| x.3 < 40.0)
            .min_by(|a, b| a.3.total_cmp(&b.3));
        if let Some((e, at, r, _)) = near {
            *tested = true;
            interact.0 = Some(e);
            if let Some(p) = crate::chests::path_near(&nav.0, &mut scratch, ptf.translation, at, r)
            {
                path.points = p;
            }
        }
    }
    let Some(e) = interact.0 else { return };
    let Ok((_, gt, mut sh)) = shrines.get_mut(e) else {
        return;
    };
    let at = gt.translation();
    // Use range (`player_orders::usable_range`): player radius + shrine radius + `usableRange`, 3D, +1 m at the
    // walk's end.
    let range = gd_game_core::player_orders::usable_range(
        pcb.radius,
        sh.radius,
        sh.range,
        path.points.is_empty(),
    );
    if ptf.translation.distance(at) > range {
        return;
    }
    interact.0 = None;
    path.points.clear();
    let mut say = |t: &str| {
        info!("devotion: {t}");
        notify.write(gd_game_core::story::Notify(t.to_owned()));
    };
    if s.restored.contains(&sh.uid) || s.totems_done.contains(&sh.uid) {
        say("tagShrineActivated");
        return;
    }
    if let Some(tag) = &sh.locked {
        say(if tag.is_empty() {
            "tagShrineLocked"
        } else {
            tag
        });
        // The player's locked-shrine voice (`char_sounds::locked_vox`, `Player::PlayLockedShrineVox`).
        if let Some(pak) =
            crate::char_sounds::locked_vox(&data, ch, crate::char_sounds::LockedKind::Shrine)
        {
            sound.write(gd_game_core::visuals::PlaySound {
                pak,
                at: Some(ptf.translation),
            });
        }
        return;
    }
    if sh.fight.is_some() {
        return;
    }
    if sh.totem {
        fights.start(&mut commands, &mut sh, at);
        return;
    }
    info!(
        "devotion: shrine window opened ({})",
        if sh.proxy.is_some() {
            "proxy"
        } else {
            "offerings"
        }
    );
    window.set(Some(e), sh.proxy.is_some());
}

/// What starting a shrine's proxy fight needs.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ShrineFights<'w> {
    // Option (SystemParam): `start` does nothing without rules; the progress (else level 1) and the factions are
    // optional input.
    progress: Option<Res<'w, gd_game_core::progress::PlayerProgress>>,
    rules: Option<Res<'w, gd_game_core::rules::CombatRules>>,
    world: Res<'w, gd_game_core::levels::Levels>,
    nav: Res<'w, Nav>,
    data: Res<'w, GameData>,
    bodies: ResMut<'w, gd_game_core::creature::BodyCache>,
    session: crate::session::Session<'w>,
    factions: Option<Res<'w, crate::factions::PlayerFactions>>,
    bosses: ResMut<'w, crate::monsters::HeroBosses>,
}

impl ShrineFights<'_> {
    /// `RunProxy`: spawns the shrine's `<difficulty>MonsterSpawn` proxy.
    fn start(&mut self, commands: &mut Commands, sh: &mut Shrine, at: Vec3) {
        let (Some(proxy), Some(rules)) = (sh.proxy.clone(), self.rules.as_deref()) else {
            return;
        };
        let player_level = self
            .progress
            .as_ref()
            .map_or(1, |p| p.progress.level)
            .max(1);
        let id = sh.uid[0] | 0x8000_0000;
        let mobs = crate::monsters::spawn_story_proxy(
            commands,
            &self.data,
            &mut self.bodies,
            &self.nav,
            rules,
            Some(&*self.world),
            &proxy,
            at,
            player_level,
            &self.session,
            self.factions.as_deref(),
            id,
            &[],
            None,
            false,
            &mut self.bosses,
        );
        info!("devotion: shrine fight, {} monsters", mobs.len());
        sh.fight = Some(mobs);
    }
}

/// The shrine window's requests: "Offer" takes one of each offering (only
/// when every slot is filled) and restores the shrine, "Start" starts a
/// proxy shrine's fight, Cancel closes it. The window also closes when the
/// player walks farther than the shrine's `uiCloseDistance` or the shrine
/// leaves the world.
#[allow(clippy::too_many_arguments)]
pub fn shrine_requests(
    mut commands: Commands,
    mut requests: MessageReader<ShrineRequest>,
    mut window: ResMut<ShrineWindow>,
    mut shrines: Query<(&GlobalTransform, &mut Shrine)>,
    players: Query<&Transform, With<Player>>,
    s: Res<DevotionState>,
    // Option (stash): without it the offerings are counted and taken from the bags alone.
    (mut inv, mut stash): (
        ResMut<gd_game_core::inventory::Inventory>,
        Option<ResMut<gd_game_core::stash::Stash>>,
    ),
    mut cleansed: MessageWriter<ShrineCleansed>,
    mut fights: ShrineFights,
) {
    let Some(e) = window.shrine else {
        requests.clear();
        return;
    };
    let (Ok(ptf), Ok((gt, mut sh))) = (players.single(), shrines.get_mut(e)) else {
        requests.clear();
        window.set(None, false);
        return;
    };
    if s.restored.contains(&sh.uid) || sh.fight.is_some() {
        requests.clear();
        window.set(None, false);
        return;
    }
    let at = gt.translation();
    for r in requests.read() {
        if window.shrine.is_none() {
            break;
        }
        match r {
            ShrineRequest::Cancel => window.set(None, false),
            ShrineRequest::Accept if window.proxy => {
                window.set(None, false);
                fights.start(&mut commands, &mut sh, at);
            }
            ShrineRequest::Accept => {
                let slots = offering_slots(&sh, &inv, stash.as_deref());
                if slots.iter().flatten().any(|(_, have)| !have) {
                    info!("devotion: shrine offerings missing");
                    continue;
                }
                for (rec, _) in slots.iter().flatten() {
                    match stash.as_deref_mut() {
                        Some(st) => {
                            gd_game_core::stash::take(std::slice::from_ref(rec), 1, &mut inv, st)
                        }
                        None => {
                            inv.remove_by_record(rec, 1);
                        }
                    }
                }
                window.set(None, false);
                cleansed.write(ShrineCleansed(e));
            }
        }
    }
    if window.shrine.is_some()
        && ptf.translation.distance(at)
            > crate::conversation::close_distance(fights.data.db.get(&sh.record))
    {
        debug!("devotion: shrine window closed (walked away)");
        window.set(None, false);
    }
}

/// Marks the shrine restored and grants its points; returns the notice.
fn restore(
    s: &mut DevotionState,
    sh: &Shrine,
    rules: &gd_game_core::rules::CombatRules,
) -> Option<gd_game_core::story::CinematicText> {
    if sh.totem {
        // `GameEngine::HandleShrineReward`: a `MonsterShrine` shows nothing.
        s.totems_done.push(sh.uid);
        s.version += 1;
        info!("devotion: monster totem {} cleared", sh.record);
        return None;
    }
    s.restored.push(sh.uid);
    if !s.discovered.contains(&sh.uid) {
        s.discovered.push(sh.uid);
    }
    let add = s.grant(sh.points, rules);
    info!(
        "devotion: shrine {} restored, +{add} point(s) ({} unspent, {} total)",
        sh.record, s.points, s.total
    );
    // `GameEngine::HandleShrineReward`: type 4, `tagDevotionShrine` over
    // `tagDevotionPoint(s)Awarded` (none when no point was granted).
    Some(gd_game_core::story::CinematicText {
        head: "tagDevotionShrine".into(),
        sub: match add {
            0 => String::new(),
            1 => "tagDevotionPointAwarded".into(),
            _ => "tagDevotionPointsAwarded".into(),
        },
        sub_args: vec![gd_game_core::floating::TextArg::Num(add as f32)],
        kind: 4,
        front: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn give_reward(
    commands: &mut Commands,
    data: &GameData,
    nav: &Nav,
    rewards: &mut MessageWriter<gd_game_core::story::StoryEffect>,
    idb: &ItemDb,
    sh: &Shrine,
    (at, rot): (Vec3, Quat),
    player_level: i32,
    difficulty: usize,
    world: Option<&gd_game_core::levels::Levels>,
    container: &mut crate::loot::ContainerLoot,
    bodies: &mut gd_game_core::creature::BodyCache,
) {
    if sh.xp > 0 {
        rewards.write(gd_game_core::story::StoryEffect(
            gd_story::story::Effect::GiveExperience(sh.xp),
        ));
    }
    let Some(loot) = &sh.loot else { return };
    let Some(rec) = data.db.get(loot) else { return };
    // PORT: the shrine's region is not carried here (engine source not traced): position lookup.
    let limits = world.and_then(|w| w.level_limits(None, at, difficulty));
    let level = gd_sim::items::loot::container_level(rec, player_level, difficulty, limits, 0);
    let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    let drops = container.drops(idb, loot, level, player_level as u32, Some(at), &mut rng);
    info!(
        "devotion: shrine loot (level {level}): {} items",
        drops.len()
    );
    // `StaticShrine::CleanseShrine` (`StaticShrine.dropper`: toss, one per release) from
    // `StaticShrine::GetLootDropCoords` / `GetLootDropRadius` (`crate::loot` doc).
    let srec = data.db.get(&sh.record);
    let body = srec
        .as_ref()
        .and_then(|r| r.str("mesh"))
        .and_then(|m| bodies.get(data, m));
    let drop = crate::loot::fixed_drop_point(nav, at, rot, body.as_deref());
    let radius = srec
        .and_then(|r| r.f32("actorRadius"))
        .unwrap_or(gd_game_core::combat::ACTOR_RADIUS);
    crate::loot::drop_loot_by(
        commands,
        idb,
        drop,
        drops.iter().map(|d| d.to_item()),
        &[],
        crate::loot::Dropper::one_by_one(radius),
    );
}

/// Restores a shrine whose offerings were taken ([`ShrineCleansed`]) or
/// whose fight ended (all its monsters dead, `AllKilled`), and pays it.
#[allow(clippy::too_many_arguments)]
pub fn shrine_fights(
    mut commands: Commands,
    rules: Res<gd_game_core::rules::CombatRules>,
    mut shrines: Query<(Entity, &GlobalTransform, &mut Shrine)>,
    alive: Query<
        (),
        (
            With<gd_game_core::combat::Combatant>,
            Without<gd_game_core::combat::Dead>,
        ),
    >,
    mut s: ResMut<DevotionState>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
    // Option: the loot's player level is 1 without it.
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    world: Res<gd_game_core::levels::Levels>,
    nav: Res<Nav>,
    data: Res<GameData>,
    mut rewards: MessageWriter<gd_game_core::story::StoryEffect>,
    mut loot: crate::loot::ContainerLoot,
    mut cleansed: MessageReader<ShrineCleansed>,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
    mut stats: MessageWriter<crate::play_stats::PlayStat>,
) {
    let offered: Vec<Entity> = cleansed.read().map(|c| c.0).collect();
    for (e, gt, mut sh) in &mut shrines {
        if let Some(mobs) = &sh.fight {
            if mobs.iter().any(|m| alive.contains(*m)) {
                continue;
            }
            sh.fight = None;
        } else if !offered.contains(&e) || s.restored.contains(&sh.uid) {
            continue;
        }
        let reward = restore(&mut s, &sh, &rules);
        let player_level = progress.as_ref().map_or(1, |p| p.progress.level).max(1);
        give_reward(
            &mut commands,
            &data,
            &nav,
            &mut rewards,
            &data.item_db(),
            &sh,
            (gt.translation(), gt.rotation()),
            player_level,
            s.difficulty,
            Some(&*world),
            &mut loot,
            &mut bodies,
        );
        if let Some(text) = reward {
            cinematic.write(text);
            stats.write(crate::play_stats::PlayStat::ShrineRestored);
        }
    }
}

/// Save block 17 lists for this difficulty: [restored, discovered].
pub fn shrine_lists(s: &DevotionState) -> [Vec<[u8; 16]>; 2] {
    [
        s.restored.iter().map(uid_bytes).collect(),
        s.discovered.iter().map(uid_bytes).collect(),
    ]
}
