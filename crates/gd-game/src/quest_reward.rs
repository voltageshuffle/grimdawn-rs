//! The Quest Complete / Quest Progress window's content (`QuestRewardWindow`; drawn by `gd_ui::questpopup`).
//!
//! Rules:
//! - trigger: `Quest2Task::Start` of an available task whose start events have reward actions and
//!   `Quest2Task::Complete(true)` of a task whose reward events have them queue the task; `Quest2::Update` sends
//!   GameEvent 43, which `QuestRewardWindow::HandleGameEvent` turns into rewards: for a task not complete its
//!   incentive events that hold, for a complete one its objectives and reward events that hold; their actions
//!   filtered by `QuestRewardWindow::IsShownReward` (`ScriptableAction::IsReward`).
//! - the rebuild (`QuestRewardWindow::Rebuild`) sums money, levels, skill points, attribute points and devotion into
//!   one icon each, gathers the faction changes into one, makes an item icon per given item, sums the experience
//!   into the XP line, and skips the skill / attribute points while the difficulty was skipped with an unlock item
//!   (`PlayStats::GetDifficultySkip`); tribute has no icon. Icon order: money, faction, level, skill points,
//!   attribute points, devotion, then the items; at most 6 are shown (the rest logged). Nothing to show (no XP, no
//!   icon): no window. The title is `tagQuestRewardWindowTitle` when the quest is complete
//!   (`Quest2::IsComplete(true)`), else `tagQuestRewardWindowInterim`.
//! - the items' roll: `Quest2Event::Execute` runs every action's `OnActivate`, `Execute`, `OnDeactivate`; the
//!   window event is dispatched by the next `Quest2::Update`, so the rebuild sees finished rolls.
//!   `ScriptableAction_GiveRandomItem::OnActivate` (shared with `GiveItem`) rolls `count` items with the action's
//!   own RNG and `Execute` gives exactly those. GiveRandomItem shows one icon per rolled item (a stack size above 1
//!   drawn at the lower right), GiveItem the first info item with the action's count.
//!
//! PORT: the port reads the frame's `QuestTaskStarted` / `QuestTaskCompleted` story effects; a start counts whenever
//! its incentives hold reward actions (the engine's "was available" test is not visible in the effect), and so does
//! every completion, including `Complete(false)` (which queues nothing in the engine). A GiveRandomItem's items are
//! rolled by `crate::loot::story_rewards` when its effect runs and handed over through [`RandomRewardRolls`]: the
//! same items the player received. Every shipped GiveRandomItem action is a reward event, so its roll and its
//! task's completion share the frame; a table's batches match its actions in order. A GiveItem icon is its record
//! with seed 1.
use bevy::prelude::*;
use gd_formats::qst::Action;
use gd_formats::save::item::Item;
use gd_game_core::story::{StoryEffect, StoryRuntime};
use gd_story::story::Effect;
use gd_story::story::Story;

/// One reward icon.
#[derive(Clone, Debug, PartialEq)]
pub enum RewardIcon {
    Money(u32),
    /// (faction record, change) lines.
    Faction(Vec<(String, i32)>),
    Level(u32),
    SkillPoints(u32),
    AttributePoints(u32),
    Devotion(u32),
    /// `GiveItem`'s record with the action's count (seed 1), or one `GiveRandomItem` roll.
    Item {
        item: Box<Item>,
    },
}

/// The window's content for one quest's tasks of a frame.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct QuestRewardShown {
    /// The quest's name.
    pub quest: String,
    /// `Quest2::IsComplete(true)`: picks the title.
    pub complete: bool,
    pub xp: u32,
    pub icons: Vec<RewardIcon>,
}

/// The frame's `GiveRandomItem` rolls: `crate::loot::story_rewards` pushes each effect's batch, [`show_rewards`]
/// matches them to the reward actions, a table's oldest batch first.
#[derive(Resource, Default)]
pub struct RandomRewardRolls(Vec<(String, Vec<Item>)>);

impl RandomRewardRolls {
    pub fn push(&mut self, table: &str, items: Vec<Item>) {
        self.0.push((table.to_owned(), items));
    }

    /// The oldest batch rolled for `table`, if any.
    pub fn take(&mut self, table: &str) -> Option<Vec<Item>> {
        let i = self.0.iter().position(|(t, _)| t == table)?;
        Some(self.0.remove(i).1)
    }

    /// Drops the batches no reward action claimed (a conversation's rolls, an unshown task's).
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// The window's icon capacity (`QuestRewardWindow.icons`).
pub const CAPACITY: usize = 6;

/// `QuestRewardWindow::IsShownReward`'s filter.
fn is_shown_reward(a: &Action) -> bool {
    match a {
        Action::GiveMoney { amount } => *amount > 0,
        Action::GiveItem { count, .. } => *count > 0,
        Action::GiveRandomItem { count, .. } => *count > 0,
        Action::GiveSkillPoint { amount } | Action::GiveAttribPoint { amount } => *amount != 0,
        Action::GiveFaction { .. }
        | Action::GiveLevel { .. }
        | Action::GiveExperience { .. }
        | Action::GiveDevotion { .. }
        | Action::GiveTribute { .. } => true,
        _ => false,
    }
}

/// `HasRewardActions` over events.
fn has_rewards(events: &[gd_formats::qst::Event]) -> bool {
    events.iter().flat_map(|e| &e.actions).any(is_shown_reward)
}

/// `QuestRewardWindow::HandleGameEvent` + `QuestRewardWindow::Rebuild` for the tasks `uids` of quest `qi`.
pub fn rewards(
    story: &Story,
    qi: usize,
    uids: &[u32],
    rolls: &mut RandomRewardRolls,
) -> Option<QuestRewardShown> {
    let quest = &story.quests.defs.list[qi].quest;
    let mut actions: Vec<&Action> = Vec::new();
    for &uid in uids {
        let Some(ti) = story.quests.task_index(qi, uid) else {
            continue;
        };
        let task = &quest.tasks[ti];
        let holds = |e: &gd_formats::qst::Event| story.eval_conditions(e.conditions.as_ref(), &[]);
        if story.quests.task_state(qi, ti) != gd_story::story::quest::STATE_COMPLETE {
            actions.extend(
                task.incentives
                    .iter()
                    .filter(|e| holds(e))
                    .flat_map(|e| &e.actions),
            );
        } else {
            for (oi, o) in task.objectives.iter().enumerate() {
                let counters = &story.quests.rt[qi].tasks[ti].objectives[oi].counters;
                if story.eval_conditions(o.conditions.as_ref(), counters) {
                    actions.extend(&o.actions);
                }
            }
            actions.extend(
                task.rewards
                    .iter()
                    .filter(|e| holds(e))
                    .flat_map(|e| &e.actions),
            );
        }
    }
    let d = story.difficulty();
    let points_skipped = d <= 1 && story.skipped(d);
    let (mut money, mut level, mut skill, mut attrib, mut devotion, mut xp) = (0, 0, 0, 0, 0, 0);
    let (mut factions, mut items) = (Vec::new(), Vec::new());
    let (mut has_money, mut has_level, mut has_skill, mut has_attrib, mut has_devotion) =
        (false, false, false, false, false);
    for a in actions.into_iter().filter(|a| is_shown_reward(a)) {
        match a {
            Action::GiveMoney { amount } => (money, has_money) = (money + amount, true),
            Action::GiveFaction { faction, amount } => factions.push((faction.clone(), *amount)),
            Action::GiveItem { item, count } => items.push(RewardIcon::Item {
                item: Box::new(Item {
                    base_name: item.clone(),
                    seed: 1,
                    stack_count: *count as u32,
                    ..Default::default()
                }),
            }),
            Action::GiveRandomItem { table, .. } => {
                if let Some(batch) = rolls.take(table) {
                    items.extend(batch.into_iter().map(|item| RewardIcon::Item {
                        item: Box::new(item),
                    }));
                }
            }
            Action::GiveLevel { amount } => (level, has_level) = (level + amount, true),
            Action::GiveExperience { amount } => xp += amount,
            Action::GiveSkillPoint { amount } if !points_skipped => {
                (skill, has_skill) = (skill + amount, true)
            }
            Action::GiveAttribPoint { amount } if !points_skipped => {
                (attrib, has_attrib) = (attrib + amount, true)
            }
            Action::GiveDevotion { amount } => (devotion, has_devotion) = (devotion + amount, true),
            _ => {}
        }
    }
    let mut icons = Vec::new();
    if has_money {
        icons.push(RewardIcon::Money(money));
    }
    if !factions.is_empty() {
        icons.push(RewardIcon::Faction(factions));
    }
    if has_level {
        icons.push(RewardIcon::Level(level));
    }
    if has_skill {
        icons.push(RewardIcon::SkillPoints(skill));
    }
    if has_attrib {
        icons.push(RewardIcon::AttributePoints(attrib));
    }
    if has_devotion {
        icons.push(RewardIcon::Devotion(devotion));
    }
    icons.extend(items);
    if icons.len() > CAPACITY {
        warn!(
            "QuestReward window, too many rewards to display.  Rewards: {}, capacity {CAPACITY}.",
            icons.len()
        );
        icons.truncate(CAPACITY);
    }
    if xp == 0 && icons.is_empty() {
        return None;
    }
    let complete = (0..quest.tasks.len())
        .all(|ti| story.quests.task_state(qi, ti) == gd_story::story::quest::STATE_COMPLETE);
    Some(QuestRewardShown {
        quest: quest.name().to_string(),
        complete,
        xp,
        icons,
    })
}

/// The frame's started / completed tasks with rewards become [`QuestRewardShown`]s, one per quest.
pub fn show_rewards(
    // Option: no story outside a session (the frame's effects and rolls are dropped).
    rt: Option<NonSend<StoryRuntime>>,
    mut effects: MessageReader<StoryEffect>,
    mut out: MessageWriter<QuestRewardShown>,
    mut rolls: ResMut<RandomRewardRolls>,
) {
    let Some(rt) = rt else {
        effects.clear();
        rolls.clear();
        return;
    };
    let story = rt.story.borrow();
    let defs = &story.quests.defs;
    let mut queued: Vec<(usize, Vec<u32>)> = Vec::new();
    for StoryEffect(e) in effects.read() {
        let (quest, task, start) = match e {
            Effect::QuestTaskStarted { quest, task } => (*quest, *task, true),
            Effect::QuestTaskCompleted { quest, task } => (*quest, *task, false),
            _ => continue,
        };
        let Some(qi) = defs.by_id(quest) else {
            continue;
        };
        let Some(ti) = story.quests.task_index(qi, task) else {
            continue;
        };
        let t = &defs.list[qi].quest.tasks[ti];
        if !has_rewards(if start { &t.incentives } else { &t.rewards }) {
            continue;
        }
        match queued.iter_mut().find(|(q, _)| *q == qi) {
            Some((_, uids)) => uids.push(task),
            None => queued.push((qi, vec![task])),
        }
    }
    for (qi, uids) in queued {
        if let Some(r) = rewards(&story, qi, &uids, &mut rolls) {
            out.write(r);
        }
    }
    rolls.clear();
}

pub struct QuestRewardPlugin;

impl Plugin for QuestRewardPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<QuestRewardShown>()
            .init_resource::<RandomRewardRolls>()
            .add_systems(
                Update,
                show_rewards
                    .after(gd_game_core::story::StoryStage::Tick)
                    .after(crate::loot::story_rewards),
            );
    }
}
