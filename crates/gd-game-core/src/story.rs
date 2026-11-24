//! The story runtime's data: the effect messages gameplay consumes, the
//! story actor components, the runtime resource (quests, conversations, Lua
//! scripts) and the pure story reads. `gd-game` adds the systems
//! (`gd_game::story`).
use crate::data::GameData;
use crate::story_env::DataEnv;
use bevy::prelude::*;
use gd_story::story::lua::{ScriptEnv, ScriptHost};
use gd_story::story::{Effect, Story};
use std::cell::RefCell;
use std::rc::Rc;

/// A story effect for other plugins (progress: [`crate::progress`]; items:
/// `gd_game::loot::story_rewards`).
#[derive(Message, Clone, Debug)]
pub struct StoryEffect(pub Effect);

/// A notice for the player (a text tag or text): the one way into the HUD's notification queue
/// (`gd_game::notify::queue_notices` reads it after the frame's writers).
#[derive(Message, Clone, Debug)]
pub struct Notify(pub String);

/// `Engine::ShowCinematicText(headline, subline, type, colour, pushFront)` (`Engine.dll`): the
/// announcement the UI shows at the top of the screen (`ui/cinematic.rs`). Types (the callers):
/// 1 achievement, 2 new mutators / formula learned, 3 reputation, 4 devotion, 5 level up / hero killed / lore bonus,
/// 6 quest update, 7 difficulty unlocked, 8 transmute, 9 Crucible / Shattered Realm bonus and script texts,
/// 10 super boss / nemesis spawned (pushed to the front).
#[derive(Message, Clone, Debug)]
pub struct CinematicText {
    pub head: String,
    pub sub: String,
    /// `{%d0}`, `{%d1}` ... of the subline's tag.
    pub sub_args: Vec<crate::floating::TextArg>,
    pub kind: u32,
    pub front: bool,
}

impl CinematicText {
    /// A headline alone (no subline), not pushed to the front.
    pub fn plain(head: String, kind: u32) -> Self {
        Self {
            head,
            sub: String::new(),
            sub_args: Vec::new(),
            kind,
            front: false,
        }
    }
}

/// The main player casts a skill record at once on himself (`ActivateSkillConfigCmd` with the player as caster and
/// target: the Shattered Realm's buff shrines, `gd_game::endless::use_shrine`), the way `gd_game::story::cast_story_skills`
/// casts a `CastSkill` from its NPC.
#[derive(Message, Clone, Debug)]
pub struct PlayerSkillCast(pub String);

/// A monster spawned by a script-placed proxy (despawned with the proxy). One whose record has hooks is a story
/// object too ([`StoryActor`], `gd_game::story_world::hooked_characters`), armed by its proxy's spawn.
#[derive(Component)]
pub struct StoryProxyChild(pub u32);

/// The creature of a story object (NPC or scripted monster), by its id.
/// The game spawns it as the object streams in; `gd_game::story::arm_story_characters`
/// makes hostile ones fight.
#[derive(Component)]
pub struct StoryActor(pub u32);

/// The player's story id.
pub const PLAYER_ID: u32 = 0x3fff_ffff;

/// The story runtime (quests, conversations, Lua scripts); `!Send` (Lua VM),
/// so a non-send resource. `notices`: the HUD's notification queue ([`crate::notify`]).
pub struct StoryRuntime {
    pub story: Rc<RefCell<Story>>,
    pub env: Rc<DataEnv>,
    pub host: ScriptHost,
    pub notices: crate::notify::NotificationQueue,
}

impl StoryRuntime {
    /// Queues a HUD notification (`GameEngine::AddUINotification`): a text tag (localised) or plain text. It empties
    /// the queue first, as every reversed error sender does (`crate::notify` module doc). Systems do not call
    /// it: they write a [`Notify`], which `gd_game::notify::queue_notices` passes here.
    pub fn notify(&mut self, tag: &str) {
        let text = self.env.localize(tag).unwrap_or_else(|| tag.to_owned());
        self.notices
            .push(gd_formats::text::strip_markup(&text), true);
    }
}

/// The story was set up (once the session's character is known).
#[derive(Resource)]
pub struct StorySetUp;

/// A story NPC's own `Skill_AttackRadius` cast in progress (`gd_game::story::cast_story_skills`): fired by
/// `gd_game::story::fire_npc_casts`.
#[derive(Component, Clone, Debug)]
pub struct NpcCast {
    pub skill: String,
    pub level_index: usize,
}

impl NpcCast {
    /// The level's `radiusEffectName` (the last one past the list).
    pub fn effect(&self, data: &GameData) -> Option<String> {
        let rec = data.db.get(&self.skill)?;
        rec.strs_iter("radiusEffectName")
            .nth(self.level_index)
            .or_else(|| rec.strs_iter("radiusEffectName").last())
            .filter(|f| !f.is_empty())
            .map(|f| f.to_string())
    }
}

/// The level of `skill` in a character record's skill list (`SkillManager::LoadSkills`:
/// `skillName{n}` / `skillLevel{n}`, 17 slots for an `Npc` (`Npc::Load`), 34 for the other classes (`Monster::Load`),
/// stopping at the first empty name; a slot with no level is not loaded). None: the character lacks the skill
/// (`FindSkillId` fails). PORT: the level is the slot's number (an equation, which no stock NPC holds, counts as 1).
pub fn owner_skill_level(data: &GameData, record: &str, skill: &str) -> Option<u32> {
    let rec = data.db.get(record)?;
    let slots = if rec.class == "Npc" { 17 } else { 34 };
    (1..=slots)
        .map_while(|i| {
            rec.str(&format!("skillName{i}"))
                .filter(|n| !n.is_empty())
                .map(|n| (i, n))
        })
        .find(|(_, n)| n.eq_ignore_ascii_case(skill))
        .and_then(|(i, _)| {
            let v = rec.get(&format!("skillLevel{i}"))?.first()?;
            Some(v.as_f32().map_or(1, |l| l.max(1.0) as u32))
        })
}

/// The story's per-frame stages: `Sync` (the inventory's items into the scripts' state), `Tick` (the scripts run), `Arm` (the story
/// characters get their fighting state). Other systems order against the stage, not the function.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryStage {
    Sync,
    Tick,
    Arm,
}

/// An objective's shown text: its plain text (`{^n}` / `^n` become line breaks) with the first condition's
/// count (`Story::objective_progress`) through that condition's tag (`tagConditionHasKilled` /
/// `tagConditionHasItem`: "{%t0} ({%d1}/{%d2})"). The quest window's objective lines read the same.
pub fn objective_text(
    tags: Option<&gd_formats::text::TagTable>,
    story: &Story,
    text: String,
    (qi, ti, oi): (usize, usize, usize),
    o: &gd_formats::qst::Objective,
) -> String {
    let text = text.replace("{^n}", "\n").replace("^n", "\n");
    let Some((done, need)) = story.objective_progress(qi, ti, oi) else {
        return text;
    };
    let tag = match o
        .conditions
        .as_ref()
        .and_then(|c| c.list.first())
        .map(|c| c.kind)
    {
        Some(9) => "tagConditionHasItem",
        _ => "tagConditionHasKilled",
    };
    let fmt = tags
        .and_then(|t| t.get(tag))
        .unwrap_or("{%t0} ({%d1}/{%d2})");
    // The loaded tag texts may have lost the braces.
    let (d, n) = (done.to_string(), need.to_string());
    fmt.replace("{%t0}", "%t0")
        .replace("{%d1}", "%d1")
        .replace("{%d2}", "%d2")
        .replace("%t0", &text)
        .replace("%d1", &d)
        .replace("%d2", &n)
}
