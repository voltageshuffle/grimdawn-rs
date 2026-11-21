//! The story runtime (quests, conversations, Lua scripts) and its effects for
//! gameplay. The game streams story objects in from the map, shows the
//! actors and dialogs, and sends the player's choices.
use crate::data::GameData;
use bevy::prelude::*;
use gd_sim::story::env::DataEnv;
use gd_sim::story::lua::{ScriptEnv, ScriptHost};
use gd_sim::story::{Effect, Player as StoryPlayer, Story};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// A story effect for other plugins (progress: [`crate::progress`]; items:
/// [`crate::loot::story_rewards`]).
#[derive(Message, Clone, Debug)]
pub struct StoryEffect(pub Effect);

/// A notice for the player (a text tag or text): the one way into the HUD's notification queue
/// (`crate::notify::queue_notices` reads it after the frame's writers).
#[derive(Message, Clone, Debug)]
pub struct Notify(pub String);

/// `Engine::ShowCinematicText(headline, subline, type, colour, pushFront)` (`Engine.dll` @0x180075e90): the
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

pub struct StoryEffectsPlugin;

impl Plugin for StoryEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<StoryEffect>()
            .add_message::<Notify>()
            .add_message::<CinematicText>()
            .add_message::<PlayerSkillCast>()
            .add_systems(PostUpdate, crate::notify::queue_notices);
    }
}

/// The main player casts a skill record at once on himself (`ActivateSkillConfigCmd` with the player as caster and
/// target: the Shattered Realm's buff shrines, `endless::use_shrine`), the way [`cast_story_skills`] casts a
/// `CastSkill` from its NPC.
#[derive(Message, Clone, Debug)]
pub struct PlayerSkillCast(pub String);

/// A monster spawned by a script-placed proxy (despawned with the proxy). One whose record has hooks is a story
/// object too ([`StoryActor`], `crate::story_world::hooked_characters`), armed by its proxy's spawn.
#[derive(Component)]
pub struct StoryProxyChild(pub u32);

/// The creature of a story object (NPC or scripted monster), by its id.
/// The game spawns it as the object streams in; [`arm_story_characters`]
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
    /// it: they write a [`Notify`], which `crate::notify::queue_notices` passes here.
    pub(crate) fn notify(&mut self, tag: &str) {
        let text = self.env.localize(tag).unwrap_or_else(|| tag.to_owned());
        self.notices
            .push(gd_formats::text::strip_markup(&text), true);
    }
}

/// The story was set up (once the session's character is known).
#[derive(Resource)]
pub struct StorySetUp;

pub struct StoryPlugin;

impl Plugin for StoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (
                    setup_story
                        .run_if(resource_exists::<crate::session::SessionReady>)
                        .run_if(not(resource_exists::<StorySetUp>)),
                    (
                        sync_items
                            .in_set(StoryStage::Sync)
                            .run_if(resource_exists::<crate::inventory::Inventory>),
                        mirror_story_monsters.in_set(StoryStage::Sync),
                        count_story_kills.in_set(StoryStage::Sync),
                        tick_story.in_set(StoryStage::Tick),
                        pick_up_story_essences,
                        quest_updates,
                    )
                        .chain(),
                )
                    .chain(),
                (
                    arm_story_characters
                        .in_set(StoryStage::Arm)
                        .run_if(resource_exists::<crate::rules::CombatRules>),
                    play_story_animations,
                    cast_story_skills.run_if(resource_exists::<crate::rules::CombatRules>),
                    fire_npc_casts,
                )
                    .after(tick_story),
            ),
        );
    }
}

/// Every monster death goes to the quest kill counters (`gd_sim::story::Kill`, types 10 / 16 / 28 / 29): the
/// killer's credit is [`crate::combat::KillCredit`] (the player or his pet, the death handlers' `GetLeader` rule),
/// and a monster of the Shattered Realm's running dungeon (`InEndlessDungeon`) never counts. PORT: the dungeon
/// test is the monster's proxy being one the running dungeon activated (like `progress::kill_experience`); the
/// engine tests the monster's region when it enters the world.
pub fn count_story_kills(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    mut kills: MessageReader<crate::combat::MonsterKilled>,
    credit: crate::combat::KillCredit,
    monsters: Query<&crate::monsters::Monster>,
) {
    let Some(rt) = rt else {
        kills.clear();
        return;
    };
    let mut story = rt.story.borrow_mut();
    for k in kills.read() {
        let in_dungeon = monsters.get(k.entity).is_ok_and(|m| {
            story
                .endless
                .activated
                .iter()
                .any(|(_, key)| *key == (m.slot.0, m.slot.1))
        });
        story.on_kill(&gd_sim::story::Kill {
            by_player: credit.of(k),
            in_dungeon,
            ..gd_sim::story::Kill::of_record(&data.db, &k.record)
        });
    }
}

/// A quest action `CastSkill` (`ScriptableAction_CastSkill`: the Crucible's blessings and power-ups, the
/// devotion purchase, the Shattered Realm's rift openings) activates the skill record at once, without animation or
/// energy, like an item skill.
///
/// `ScriptableAction_CastSkill::Execute` @0x18055f890: the action's owner (a conversation's NPC) casts the
/// skill, found in its own skill list ([`owner_skill_level`]), on the main player, at the owner's level. A skill the
/// owner lacks, or an owner that is no `Character` (a quest's events, objectives and tasks run their actions with
/// no owner), does nothing. `Skill_BuffOther::ActivateNow` @0x1804e72f0: the targets are the live non-`Npc`
/// characters around the caster within the buff's `skillTargetRadius` that are friends of the caster by faction
/// (the player, the pets, every monster whose pack is not hostile to the NPC's), plus the player; each gets the
/// buff record for its `skillActiveDuration` (pets add their owner's `petBonusName`, `pets::pet_bonuses_with`).
/// PORT: the buffs fire at once (no use state or animation on the NPC); the `Skill_PetAttack` type check is not
/// made (no stock NPC lists one).
///
/// A `Skill_AttackRadius` (the Shattered Realm NPCs' `endlessdungeonactivate.dbr` / `endlessdungeondeactivate.dbr`)
/// is the NPC's own cast ([`NpcCast`], [`fire_npc_casts`]): it plays the skill's animation and at the hit point
/// (else the clip's "End") the level's radius effect starts at the NPC. PORT: its hits are not dealt (both stock
/// skills deal nothing). [`PlayerSkillCast`] is the player's own cast (the buff shrines), with the same rules.
///
/// Ordering (`ScriptableActionCollection::Execute` @0x18046e380): a step's actions run in order,
/// synchronously; in single player a `LuaScript` event dispatches at once, so a script that destroys the owner
/// (the Shattered Realm keeper's `swapDungeonNPCs`) leaves a later `CastSkill` without an owner: nothing happens.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn cast_story_skills(
    mut commands: Commands,
    mut events: MessageReader<StoryEffect>,
    data: Res<GameData>,
    rt: Option<NonSend<StoryRuntime>>,
    factions: Option<Res<crate::factions::PlayerFactions>>,
    actors: Query<(Entity, &StoryActor, &Transform)>,
    mut targets: Query<
        (
            Entity,
            &Transform,
            &crate::combat::Combatant,
            Option<&mut crate::skill_buffs::ReceivedBuffs>,
            Has<crate::player::Player>,
            crate::combat::SideData,
        ),
        (
            Without<crate::combat::Dead>,
            Without<crate::world::Breakable>,
        ),
    >,
    mut procs: MessageWriter<crate::casting::ProcCast>,
    mut player_casts: MessageReader<PlayerSkillCast>,
    mut npc_chars: Query<&mut crate::character::Character, With<StoryActor>>,
) {
    let Some((player, ptf)) = targets.iter().find(|t| t.4).map(|t| (t.0, *t.1)) else {
        return;
    };
    let casts: Vec<(String, Option<u32>)> = events
        .read()
        .filter_map(|ev| match &ev.0 {
            Effect::CastSkill { skill, caster } => match caster {
                Some(id) => Some((skill.clone(), Some(*id))),
                None => {
                    debug!("story: CastSkill {skill} without a character owner: nothing");
                    None
                }
            },
            _ => None,
        })
        .chain(player_casts.read().map(|c| (c.0.clone(), None)))
        .collect();
    for (skill, caster) in &casts {
        let Some(rec) = data.db.get(skill) else {
            continue;
        };
        // The caster: the owner's character holding the skill (`None`: the player's own cast).
        let npc = match caster {
            Some(id) => {
                let found = rt.as_ref().and_then(|rt| {
                    let record = rt.story.borrow().entities.get(*id)?.record.clone();
                    let (e, _, tf) = actors.iter().find(|(_, a, _)| a.0 == *id)?;
                    Some((record, e, tf.translation))
                });
                let Some(found) = found else {
                    debug!("story: CastSkill {skill}: owner {id} has no character in the world");
                    continue;
                };
                let Some(level) = owner_skill_level(&data, &found.0, skill) else {
                    debug!("story: CastSkill {skill}: not a skill of {}", found.0);
                    continue;
                };
                Some((found, level))
            }
            None => None,
        };
        let level_index = npc.as_ref().map_or(0, |n| n.1.saturating_sub(1) as usize);
        let npc = npc.map(|n| n.0);
        if matches!(rec.class.as_str(), "Skill_BuffOther" | "Skill_BuffRadius")
            && let Some(buff) = rec.str("buffSkillName").map(str::to_owned)
        {
            let brec = data.db.get(&buff);
            let secs = brec
                .as_ref()
                .and_then(|b| b.f32("skillActiveDuration"))
                .unwrap_or(0.0);
            let radius = brec
                .as_ref()
                .and_then(|b| b.f32("skillTargetRadius"))
                .unwrap_or(0.0);
            let (side, from, at) = match &npc {
                Some((record, e, at)) => (
                    crate::combat::Side::Monster {
                        pack: crate::monsters::FactionPack::shared(&data, record),
                        allied: crate::monsters::is_friendly(&data.db, factions.as_deref(), record),
                    },
                    *e,
                    *at,
                ),
                None => (crate::combat::Side::Player, player, ptf.translation),
            };
            info!("story: buff {buff} for {secs} s (radius {radius})");
            let mut fresh = Vec::new();
            for (t, ttf, cb, buffs, is_player, tsd) in &mut targets {
                let near = radius > 0.0 && (ttf.translation - at).length() <= radius + cb.radius;
                let friend = !side.foe(tsd.side_ref());
                // The player is the cast's target: always buffed.
                if !is_player && !(near && friend) {
                    continue;
                }
                let b = crate::skill_buffs::ReceivedBuff::new(
                    &data.db,
                    &buff,
                    skill,
                    level_index,
                    from,
                    Vec::new(),
                    secs,
                );
                let put = match buffs {
                    Some(mut l) => {
                        let put = l.land(b.clone(), |_| true);
                        crate::skill_buffs::mark_player(&mut commands, t, Some(l), is_player);
                        put
                    }
                    None => {
                        let mut l = crate::skill_buffs::ReceivedBuffs::default();
                        let put = l.land(b.clone(), |_| true);
                        fresh.push((t, l, is_player));
                        put
                    }
                };
                if put.created() {
                    crate::skill_buffs::on_created(
                        &mut commands,
                        &data,
                        &b,
                        t,
                        ttf.translation,
                        put,
                    );
                }
            }
            for (t, l, is_player) in fresh {
                commands.entity(t).try_insert(l);
                if is_player {
                    commands.entity(t).try_insert(crate::player::ProfileDirty);
                }
            }
            continue;
        }
        if rec.class == "Skill_AttackRadius"
            && let Some((_, e, at)) = &npc
        {
            let animates = npc_chars.get_mut(*e).is_ok_and(|mut c| {
                let special = rec.str("skillSpecialAnimationName").map(str::to_owned);
                c.start_cast(&rec.class, special.filter(|s| !s.is_empty()))
            });
            let cast = NpcCast {
                skill: skill.clone(),
                level_index,
            };
            info!("story: npc cast {skill} (animates: {animates})");
            if animates {
                commands.entity(*e).try_insert(cast);
            } else if let Some(fx) = cast.effect(&data) {
                crate::visuals::spawn_fx(
                    &mut commands,
                    &data,
                    &fx,
                    Transform::from_translation(*at),
                );
            }
            continue;
        }
        let kind = crate::casting::kind_of(rec);
        if kind == crate::casting::Kind::Unsupported {
            warn!("story: CastSkill {skill} ({}) not supported", rec.class);
            continue;
        }
        info!("story: cast {skill}");
        procs.write(crate::casting::ProcCast {
            caster: player,
            cast: crate::casting::Casting {
                skill: skill.clone(),
                level_index,
                kind,
                point: ptf.translation + crate::character::facing(&ptf) * 2.0,
                target: None,
                hostile: false,
                parent: None,
            },
        });
    }
}

/// A story NPC's own `Skill_AttackRadius` cast in progress ([`cast_story_skills`]): fired by [`fire_npc_casts`].
#[derive(Component, Clone, Debug)]
pub struct NpcCast {
    pub skill: String,
    pub level_index: usize,
}

impl NpcCast {
    /// The level's `radiusEffectName` (the last one past the list).
    fn effect(&self, data: &GameData) -> Option<String> {
        let rec = data.db.get(&self.skill)?;
        rec.strs_iter("radiusEffectName")
            .nth(self.level_index)
            .or_else(|| rec.strs_iter("radiusEffectName").last())
            .filter(|f| !f.is_empty())
            .map(|f| f.to_string())
    }
}

/// An [`NpcCast`] fires at its animation's hit point, else at its "End" (as `casting::fire_casts`): the radius
/// effect starts at the NPC.
pub fn fire_npc_casts(
    mut commands: Commands,
    data: Res<GameData>,
    mut events: MessageReader<crate::character::AnimCallback>,
    casts: Query<(&NpcCast, &Transform)>,
) {
    let mut fired = std::collections::HashSet::new();
    for ev in events.read() {
        if !matches!(ev.name.as_str(), "RightHandHit" | "LeftHandHit" | "End") {
            continue;
        }
        let Ok((cast, tf)) = casts.get(ev.entity) else {
            continue;
        };
        if !fired.insert(ev.entity) {
            continue;
        }
        commands.entity(ev.entity).try_remove::<NpcCast>();
        if let Some(fx) = cast.effect(&data) {
            info!("story: npc cast {} fires {fx}", cast.skill);
            crate::visuals::spawn_fx(
                &mut commands,
                &data,
                &fx,
                Transform::from_translation(tf.translation),
            );
        }
    }
}

/// The level of `skill` in a character record's skill list (`SkillManager::LoadSkills` @0x18052ef00:
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

/// `Shrine:IsRestored` reads the devotion shrines restored on this
/// difficulty (`StaticShrine::IsCleansed`).
pub fn publish_shrines(story: &mut Story, devotion: Option<&crate::devotion::DevotionState>) {
    if let Some(d) = devotion
        && story.host_facts.restored_shrines.len() != d.restored.len()
    {
        story.host_facts.restored_shrines = d.restored.iter().copied().collect();
    }
}

pub fn setup_story(world: &mut World) {
    world.insert_resource(StorySetUp);
    let t = std::time::Instant::now();
    let data = world.resource::<GameData>().clone();
    let env = Rc::new(DataEnv::new(
        data.vfs.clone(),
        data.db.clone(),
        Some(data.tags()),
    ));
    let (defs, dialogs) = match (env.quest_defs(), env.dialogs()) {
        (Ok(q), Ok(d)) => (q, d),
        (Err(e), _) | (_, Err(e)) => {
            error!("story: {e:#}");
            return;
        }
    };
    let (nq, nc) = (defs.list.len(), dialogs.defs.len());
    let choice = world.resource::<crate::session::Choice>().clone();
    let mut player = StoryPlayer::new_character(PLAYER_ID, &choice.name);
    if choice.female {
        player.record = "records/creatures/pc/femalepc01.dbr".into();
    }
    let mut story = Story::new(Arc::new(defs), dialogs, player.clone());
    // The session's `GameInfo` difficulty (0..2) and challenge (1 Veteran on Normal, 3 Ultimate Veteran on Ultimate):
    // `Game.GetGameDifficulty` / `GetGameChallenge` (sub_1400782a0 / sub_140078360 read `GameInfo::
    // GetDifficulty` / `GetChallenge`), the quests' current difficulty (set before the saved quests load).
    if let Some(m) = world.get_resource::<crate::session::GameMode>() {
        story.game.difficulty = m.0.difficulty.min(2) as u32;
        story.game.challenge = m.0.challenge;
    }
    if let Some(ch) = world.get_resource::<crate::session::LoadedCharacter>() {
        let gdc = &ch.0;
        player.name = gdc.header.name.clone();
        player.level = gdc.bio.level;
        player.experience = gdc.bio.experience;
        player.hardcore = gdc.header.hardcore != 0;
        // Header byte is `GetGender() == 1`, 1 = male
        // (`Character::Load`: "Male" → 1, "Female" → 2).
        if gdc.header.sex == 0 {
            player.record = "records/creatures/pc/femalepc01.dbr".into();
        }
        story.player = player;
        load_save_state(&mut story, &choice, gdc);
    }
    // `GD_TOKENS` (hooks.rs).
    for t in &world.resource::<crate::hooks::Hooks>().gameplay.tokens {
        story.player.tokens.insert(t.clone());
    }

    if let Some(ch) = world.get_resource::<crate::session::LoadedCharacter>() {
        let sv = &mut story.game.survival;
        sv.greatest_difficulty = ch.0.info.greatest_survival_difficulty;
        sv.greatest_wave = ch.0.stats.survival_greatest_wave;
        sv.greatest_score = ch.0.stats.survival_greatest_score;
        sv.defenses_built = ch.0.stats.survival_defenses_built;
        sv.power_ups = ch.0.stats.survival_powerups_activated;
        story.player.tribute = ch.0.info.current_tribute;
        story.player.difficulty_skip = ch.0.stats.difficulty_skip;
    }
    story.game.survival_mode = choice.survival.is_some();
    // The session's random stream (`Story::rng`): from the gameplay generator, so a seeded game repeats.
    story.rng = gd_sim::combat::Rng(crate::rng::u32(1..0x7FFF_FFFF));
    let story = Rc::new(RefCell::new(story));
    let host = match ScriptHost::new(story.clone(), env.clone() as Rc<dyn ScriptEnv>) {
        Ok(h) => h,
        Err(e) => {
            error!("story: Lua: {e}");
            return;
        }
    };
    for l in story.borrow_mut().take_log() {
        info!("story: {l}");
    }
    info!(
        "story: {nq} quests, {nc} conversations, scripts loaded in {:?}",
        t.elapsed()
    );
    world.insert_non_send(StoryRuntime {
        story,
        env,
        host,
        notices: Default::default(),
    });
}

/// The story's per-frame stages: `Sync` (the inventory's items into the scripts' state), `Tick` (the scripts run), `Arm` (the story
/// characters get their fighting state). Other systems order against the stage, not the function.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryStage {
    Sync,
    Tick,
    Arm,
}

/// The story's item counts (`HasItem` conditions, Lua `HasItem`, both `Player::GetItemCountInStashes`, see
/// [`gd_sim::story::Player::item_count_in_stashes`]) follow the player's items: the bags and the held item, not
/// the worn gear (`Inventory::GetItemCount` @0x18031f4b0 skips what `EquipmentCtrl::GetEquippedLocation`
/// finds), stacks counted; the stash sacks, the transfer stash and the blacksmith's materials go to
/// `stored_items`; the incomplete relics and charms of the bags and sacks (not the materials) to
/// `incomplete_items`. PORT: the held item counts as the inventory's (the engine's cursor item placement is not
/// traced). The bags themselves go to `bags` (`Player.IsSpaceAvailable`): each item's record, cell and footprint,
/// and whether its stack has room for one more (`crate::stack::max_of`).
pub fn sync_items(
    inv: Res<crate::inventory::Inventory>,
    // Option: the stash is optional input (before `init_stash`).
    stash: Option<Res<crate::stash::Stash>>,
    data: Res<GameData>,
    rt: Option<NonSend<StoryRuntime>>,
    (mut seen, mut fresh): (Local<u64>, crate::session::NewSession),
) {
    let Some(rt) = rt else {
        return;
    };
    if fresh.begun() {
        *seen = 0;
    }
    let stash_changed = stash.as_ref().is_some_and(|s| s.is_changed());
    if inv.version == *seen && *seen != 0 && !stash_changed {
        return;
    }
    *seen = inv.version.max(1);
    let mut incomplete: HashMap<String, u32> = HashMap::new();
    let mut add = |map: &mut HashMap<String, u32>, it: &gd_formats::save::item::Item| {
        let key = it.base_name.to_ascii_lowercase();
        let n = it.stack_count.max(1);
        let partial = data.db.get(&it.base_name).is_some_and(|r| {
            matches!(r.class.as_str(), "ItemRelic" | "ItemCharm")
                && !gd_sim::items::components::is_complete(r, it.var1)
        });
        if partial {
            *incomplete.entry(key.clone()).or_default() += n;
        }
        *map.entry(key).or_default() += n;
    };
    let mut counts: HashMap<String, u32> = HashMap::new();
    for it in inv
        .bags
        .iter()
        .flat_map(|b| b.items.iter().map(|p| &p.item))
        .chain(inv.held.iter())
    {
        add(&mut counts, it);
    }
    let mut stored: HashMap<String, u32> = HashMap::new();
    if let Some(stash) = &stash {
        for it in stash
            .pages
            .iter()
            .chain(&stash.transfer)
            .flat_map(|s| s.items.iter().map(|p| &p.item))
        {
            add(&mut stored, it);
        }
        for e in &stash.materials.entries {
            *stored.entry(e.record.to_ascii_lowercase()).or_default() += e.count;
        }
    }
    debug!(
        "story: {} item records held, {} stored, skeleton keys {}",
        counts.len(),
        stored.len(),
        counts
            .get("records/items/crafting/materials/craft_skeletonkey.dbr")
            .copied()
            .unwrap_or(0)
    );
    let mut story = rt.story.borrow_mut();
    story.player.items = counts;
    story.player.stored_items = stored;
    story.player.incomplete_items = incomplete;
    let engine = crate::stack::engine_max_in(inv.db().get(crate::stack::GAME_ITEM_INFO));
    story.player.bags = inv
        .bags
        .iter()
        .map(|b| gd_sim::items::inventory::Sack {
            width: b.width,
            height: b.height,
            vertical: b.vertical,
            items: b
                .items
                .iter()
                .map(|p| gd_sim::items::inventory::Placed {
                    item: gd_sim::story::state::BagItem {
                        record: p.item.base_name.clone(),
                        stack_room: crate::stack::count(&p.item)
                            < crate::stack::max_of(&inv, &p.item, engine),
                    },
                    x: p.x,
                    y: p.y,
                    size: p.size,
                })
                .collect(),
        })
        .collect();
}

/// The loaded character's `quests.gdd` of every difficulty (the current one last: `gd_sim::story::save`) and
/// `conversations.gdd` of this difficulty in the session's per-map folder
/// (`Choice::map_folder`: the campaign's or the Crucible arena's), read-only here; `save.rs` writes them back.
/// A Crucible arena without a file starts with no quest state; its tokens are the character's survival tokens of
/// this difficulty from `player.gdc` (shared by the arenas, [`crate::survival::load_tokens`]), not the arena file's.
fn load_save_state(
    story: &mut Story,
    choice: &crate::session::Choice,
    gdc: &gd_formats::save::gdc::Gdc,
) {
    load_map_folder(story, choice, gdc.info.difficulty());
    if choice.survival.is_some() {
        story.player.tokens = crate::survival::load_tokens(gdc, gdc.info.difficulty());
    }
}

fn load_map_folder(story: &mut Story, choice: &crate::session::Choice, difficulty: usize) {
    let Some(root) = choice.save_file(choice.map_folder()) else {
        return;
    };
    let others = (0..gd_sim::story::state::DIFFICULTIES).filter(|&d| d != difficulty);
    for d in others.chain([difficulty]) {
        let path = root
            .join(crate::fog::FogOfWar::dir_name(d))
            .join("quests.gdd");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            // A difficulty never played has no file.
            Err(e) if d != difficulty && e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                warn!("story: {}: {e}", path.display());
                continue;
            }
        };
        match gd_formats::save::gdd::Quests::parse(&bytes) {
            Ok(q) => {
                story.load_quests(&q, d);
                info!(
                    "story: loaded {} quest states, {} tokens ({})",
                    q.quests.len(),
                    q.tokens.len(),
                    crate::fog::FogOfWar::dir_name(d)
                );
            }
            Err(e) => warn!("story: {}: {e:#}", path.display()),
        }
    }
    let dir = root.join(crate::fog::FogOfWar::dir_name(difficulty));
    if let Ok(c) = std::fs::read(dir.join("conversations.gdd"))
        .map_err(anyhow::Error::from)
        .and_then(|b| gd_formats::save::gdd::Conversations::parse(&b))
    {
        story.load_conversations(&c);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn tick_story(
    rt: Option<NonSendMut<StoryRuntime>>,
    time: Res<Time>,
    mut carry: Local<f32>,
    hooks: Res<crate::hooks::Hooks>,
    (mut fresh, mut lua_ran): (crate::session::NewSession, Local<bool>),
    players: Query<(&Transform, Has<crate::combat::Dead>), With<crate::player::Player>>,
    // Option: progress and devotion are optional input (the story runs without them).
    progress: Option<Res<crate::progress::PlayerProgress>>,
    devotion: Option<Res<crate::devotion::DevotionState>>,
    mut out: MessageWriter<StoryEffect>,
    mut cinematic: MessageWriter<CinematicText>,
) {
    // The `GD_LUA` hook runs once per session.
    if fresh.begun() {
        *lua_ran = false;
    }
    let Some(mut rt) = rt else { return };
    let rt = &mut *rt;
    publish_shrines(&mut rt.story.borrow_mut(), devotion.as_deref());
    if let Ok((t, dead)) = players.single() {
        let mut s = rt.story.borrow_mut();
        s.player.position = t.translation.to_array();
        s.player.dead = dead;
        // The game's player is the source of truth for level/xp/money.
        if let Some(p) = &progress {
            s.player.level = p.progress.level.max(1) as u32;
            s.player.experience = p.progress.experience.min(u32::MAX as u64) as u32;
            s.player.money = p.gold.min(u32::MAX as u64) as u32;
        }
        // The `TotalDevotion` condition (the Crucible's devotion purchase prices) reads the points ever earned.
        if let Some(d) = &devotion {
            s.player.total_devotion = d.total;
        }
    } else {
        return; // Wait for the player before triggers can fire.
    }
    // `GD_LUA` (hooks.rs): `[@secs:]code`, parsed until it ran.
    if !*lua_ran && let Some(hook) = hooks.gameplay.lua.as_deref() {
        let (after, code) = hook
            .strip_prefix('@')
            .and_then(|h| h.split_once(':'))
            .and_then(|(s, code)| Some((s.parse::<f32>().ok()?, code)))
            .unwrap_or((0.0, hook));
        if time.elapsed_secs() >= after {
            *lua_ran = true;
            if let Err(e) = rt.host.lua.load(code).exec() {
                error!("GD_LUA: {e}");
            }
        }
    }
    rt.host.update(crate::session::frame_ms(&mut carry, &time));
    let effects = rt.story.borrow_mut().take_effects();
    for e in effects {
        match &e {
            // `Game.NotifyNewMutators` shows cinematic text `tagNewMutators` / `tagNewMutator` (type 2).
            Effect::NotifyNewMutators(plural) => {
                let tag = if *plural {
                    "tagNewMutators"
                } else {
                    "tagNewMutator"
                };
                let head = rt.env.localize(tag).unwrap_or(tag.to_owned());
                cinematic.write(CinematicText::plain(head, 2));
            }
            // The scripts' `ShowCinematicText` action: type 9 ("quest-script texts").
            // Lua `UI.Notify(tag)` (sub_140086a40 → `GameEngine::QuestCommandUiNotify`): the same, no
            // subline.
            Effect::CinematicText(tag) | Effect::Notify(tag) => {
                let head = rt.env.localize(tag).unwrap_or(tag.clone());
                cinematic.write(CinematicText::plain(head, 9));
            }
            Effect::QuestTaskStarted { .. }
            | Effect::QuestTaskCompleted { .. }
            | Effect::ObjectiveSatisfied { .. } => {
                info!("story: {e:?}");
                // The quest reward window reads the task starts / completions (`crate::quest_reward`).
                out.write(StoryEffect(e.clone()));
            }
            Effect::Unimplemented(_) => {}
            // `ScriptableAction_ScriptEvent` / `Script.Event` (`GameEngine::ScriptEvent`): it only
            // unlocks achievements (`crate::achievements::script_event_unlocks`).
            Effect::ScriptEvent(name) => {
                debug!("story: script event {name}");
                out.write(StoryEffect(e.clone()));
            }
            Effect::GiveExperience(_)
            | Effect::GiveLevels(_)
            | Effect::GiveMoney(_)
            | Effect::GiveSkillPoints(_)
            | Effect::GiveAttributePoints(_)
            | Effect::GiveItem { .. }
            | Effect::GiveRandomItem { .. }
            | Effect::Kill { .. }
            | Effect::SetDoor { .. }
            | Effect::SetLocked { .. }
            | Effect::Faction { .. }
            | Effect::UnlockFaction(_)
            | Effect::UnlockTutorial(_)
            | Effect::KillAllMonsters
            | Effect::RemoveEndlessSoulBuffs
            | Effect::RemoveEndlessShrineBuffs
            | Effect::FullscreenGlow
            | Effect::SurvivalAutosave
            | Effect::SaveSurvivalScore
            | Effect::CompleteSurvivalDifficulty
            | Effect::GiveDevotion(_)
            | Effect::UnlockNextDifficulty
            | Effect::PlaySound { .. }
            | Effect::PlayAnimation { .. }
            | Effect::CastSkill { .. }
            | Effect::UseSkill { .. }
            | Effect::MoveAction { .. }
            | Effect::LinkPatrolPointGroup { .. }
            | Effect::TeleportHome { .. }
            | Effect::DestroyItems { .. }
            | Effect::TeleportPlayer { .. } => {
                info!("story: effect {e:?}");
                out.write(StoryEffect(e.clone()));
            }
            _ => info!("story: effect {e:?}"),
        }
    }
    for l in rt.story.borrow_mut().take_log() {
        info!("story: {l}");
    }
}

/// Quest `ControllerState` actions: the conversation's NPC plays the
/// `AnimationSet_Type` slot ([`crate::character::Character::play_scripted`]).
pub fn play_story_animations(
    mut events: MessageReader<StoryEffect>,
    mut actors: Query<(&StoryActor, &mut crate::character::Character)>,
) {
    for ev in events.read() {
        let Effect::PlayAnimation { id: Some(id), anim } = &ev.0 else {
            continue;
        };
        let Some(ty) = gd_sim::anim::AnimType::from_index(*anim as usize) else {
            continue;
        };
        for (a, mut c) in &mut actors {
            if a.0 == *id {
                c.play_scripted(ty);
            }
        }
    }
}

/// The armed story monsters as the scripts see them: their position (`Entity::GetCoords` of the live character:
/// the coords a `Monsters` area trigger tests, `gd_sim::story::trigger`) and, once dead, `Character::IsAlive` false
/// (the Shattered Realm's Loghorrean room counts its three bosses dead by `Character.IsAlive`).
pub fn mirror_story_monsters(
    rt: Option<NonSend<StoryRuntime>>,
    monsters: Query<
        (&StoryActor, &Transform, Has<crate::combat::Dead>),
        With<crate::monsters::Monster>,
    >,
) {
    let Some(rt) = rt else { return };
    let mut s = rt.story.borrow_mut();
    for (&StoryActor(id), tf, dead) in &monsters {
        if let Some(o) = s.entities.get_mut(id) {
            o.coords.origin = tf.translation.to_array();
            if dead {
                o.alive = false;
            }
        }
    }
}

/// A Shattered Realm essence a script placed (`Entity.Create` + `SetCoords`: the Loghorrean room's
/// `endlessdungeon_essence_special`) is picked up when the player comes within 4 m and used at once, like a dropped
/// one (`OneShotFilter` @0x1803F27B0, the 4 m auto-pickup;
/// `endless::progress` applies its progress and soul charge).
pub fn pick_up_story_essences(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    players: Query<&Transform, With<crate::player::Player>>,
) {
    let (Some(rt), Ok(p)) = (rt, players.single()) else {
        return;
    };
    // Only the running dungeon's scripts place them.
    if rt.story.borrow().endless.dungeon.is_none() {
        return;
    }
    let mut s = rt.story.borrow_mut();
    let near: Vec<(u32, String)> = s
        .entities
        .objects
        .values()
        .filter(|o| {
            o.class == gd_sim::story::state::ObjClass::Item
                && o.in_world
                && Vec3::from(o.coords.origin).distance_squared(p.translation) <= 16.0
                && data
                    .db
                    .get(&o.record)
                    .is_some_and(|r| r.class == "OneShot_EndlessDungeon")
        })
        .map(|o| (o.id, o.record.clone()))
        .collect();
    for (id, record) in near {
        s.entities.remove(id);
        s.endless.essence_queue.push(record);
    }
}

/// A new story `Character` the player is a foe of
/// ([`crate::monsters::is_friendly`]) fights as a monster: at the script's
/// `Character.Create(record, level)` level (bounty targets), else its record's
/// level at the player's, its AI homed at the object's placed origin. A story
/// proxy's monster ([`StoryProxyChild`]) or a hooked monster another spawn armed (`crate::story_world::
/// hooked_characters`: it already has its [`crate::monsters::Monster`]) keeps what its spawn gave it.
#[allow(clippy::too_many_arguments)]
pub fn arm_story_characters(
    mut commands: Commands,
    new: Query<
        (Entity, &StoryActor),
        (
            Added<StoryActor>,
            Without<StoryProxyChild>,
            Without<crate::monsters::Monster>,
        ),
    >,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    rules: Res<crate::rules::CombatRules>,
    // Option: factions and progress are optional input (friendly check, level 1).
    factions: Option<Res<crate::factions::PlayerFactions>>,
    progress: Option<Res<crate::progress::PlayerProgress>>,
) {
    let Some(rt) = rt else {
        return;
    };
    let story = rt.story.borrow();
    for (e, &StoryActor(id)) in &new {
        let Some(o) = story
            .entities
            .get(id)
            .filter(|o| o.class == gd_sim::story::state::ObjClass::Character)
        else {
            continue;
        };
        // A friendly creature of class Monster with a controller that fights (a Crucible defense turret) is an
        // ally of the player (`combat::Allied`); the other friendly ones and every `Npc` stand as NPCs
        // (`monsters::stands_as_npc`).
        let friendly = crate::monsters::is_friendly(&data.db, factions.as_deref(), &o.record);
        if crate::monsters::stands_as_npc(&data.db, factions.as_deref(), &o.record) {
            continue;
        }
        let level = story
            .entities
            .levels
            .get(&id)
            .map(|l| *l as i32)
            .unwrap_or_else(|| {
                // No script level: `Character::Load`'s own level at the player's (`monsters::placed_char_level`). PORT: that
                // the `Character.Create` binding (untraced) keeps it for a nil level.
                let player = progress.as_ref().map_or(1, |p| p.progress.level);
                crate::monsters::placed_char_level(&data.db, &o.record, player)
            });
        let Some(p) = gd_sim::actor::monster(
            &data.db,
            &rules.0,
            &o.record,
            level,
            gd_sim::actor::Side::Monster,
        ) else {
            continue;
        };
        let radius = crate::combat::get_radius(data.db.get(&o.record), crate::combat::ACTOR_RADIUS);
        if friendly {
            debug!("story: ally {} (level {level}) {e:?}", o.record);
            commands.entity(e).insert(crate::combat::Allied);
        }
        let mut ai =
            crate::combat::MonsterAi::from_record(&data, &o.record, Vec3::from(o.coords.origin));
        // `Character.Create` enables the spawn animation (`Entities::spawn_animation`).
        if story.entities.spawn_animation.contains(&id) {
            ai.enable_spawn_animation();
        }
        commands.entity(e).insert((
            crate::combat::Combatant::new(std::sync::Arc::new(p), radius),
            ai,
            crate::monsters::Monster {
                record: o.record.clone(),
                level,
                slot: (usize::MAX, id, 0),
            },
        ));
    }
}

/// `Game.UnlockNextDifficulty` → `GameEngine::GameWon`: beating the game on a
/// difficulty at least the unlocked one (and below Ultimate) unlocks both Elite
/// and Ultimate (see `grimdawn/src/menu.rs`).
pub fn game_won(
    mut commands: Commands,
    mut events: MessageReader<StoryEffect>,
    // Option: this system creates `DifficultyUnlock` and drains the effects without it.
    unlock: Option<ResMut<crate::session::DifficultyUnlock>>,
    loaded: Option<Res<crate::session::LoadedCharacter>>,
    ready: Option<Res<crate::session::SessionReady>>,
    mut notify: MessageWriter<CinematicText>,
) {
    let Some(mut unlock) = unlock else {
        if ready.is_some() {
            commands.insert_resource(crate::session::DifficultyUnlock(
                loaded
                    .as_ref()
                    .map_or(0, |l| crate::session::unlocked(&l.0)),
            ));
        }
        events.clear();
        return;
    };
    let d = loaded.as_ref().map_or(0, |l| l.0.info.difficulty());
    for ev in events.read() {
        if !matches!(ev.0, Effect::UnlockNextDifficulty) {
            continue;
        }
        if d >= unlock.0 && d != 2 {
            unlock.0 = 2;
            info!("difficulty: Elite and Ultimate unlocked");
            // Cinematic text type 7 (difficulty unlocked).
            notify.write(CinematicText::plain("tagDifficultyUnlocked".to_owned(), 7));
        }
    }
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

/// `Quest2Objective::OnChanged` (@0x180422450): when a kill or item
/// count of an objective of a task in progress changes, `tagQuestUpdate` over the objective's line (type 6),
/// at most once per 4 s per objective. The first sight of an objective only records its count, and the first
/// 5 s of a session only record (the counts settle while the inventory arrives after the story).
/// PORT: sent here, not by the quest window's collect, so the notice shows with the window closed; the window
/// walks the quests in repository order, this in definition order (only same-frame notice order differs).
#[allow(clippy::too_many_arguments)]
pub fn quest_updates(
    mut fresh: crate::session::NewSession,
    rt: Option<NonSend<StoryRuntime>>,
    inv: Option<Res<crate::inventory::Inventory>>,
    stash: Option<Res<crate::stash::Stash>>,
    clock: Res<Time<Real>>,
    mut cinematic: MessageWriter<CinematicText>,
    mut last_count: Local<HashMap<(usize, usize, usize), u32>>,
    mut last_shown: Local<HashMap<(usize, usize, usize), f32>>,
    mut since: Local<Option<f32>>,
    mut inputs: Local<Option<(u64, u64, Option<bevy::ecs::change_detection::Tick>, usize)>>,
) {
    if fresh.begun() {
        last_count.clear();
        last_shown.clear();
        *since = None;
        *inputs = None;
    }
    let Some(rt) = rt else { return };
    // The grace period starts with the session's story, not the app (the system runs in the menu too).
    let started = *since.get_or_insert(clock.elapsed_secs());
    let story = rt.story.borrow();
    let log = &story.quests;
    // Nothing an objective counts changes without one of these moving: the log's revision, the item counts'
    // sources (the bags and the stash), the difficulty.
    let key = (
        log.revision,
        inv.as_ref().map_or(0, |i| i.version),
        stash.as_ref().map(|s| s.last_changed()),
        story.difficulty(),
    );
    if *inputs == Some(key) {
        return;
    }
    *inputs = Some(key);
    let now = clock.elapsed_secs();
    for (qi, def) in log.defs.list.iter().enumerate() {
        for (ti, (t, r)) in def.quest.tasks.iter().zip(&log.rt[qi].tasks).enumerate() {
            if r.state != gd_sim::story::quest::STATE_IN_PROGRESS {
                continue;
            }
            for (oi, o) in t.objectives.iter().enumerate() {
                let Some((done, _)) = story.objective_progress(qi, ti, oi) else {
                    continue;
                };
                let key = (qi, ti, oi);
                let changed = last_count
                    .insert(key, done)
                    .is_some_and(|prev| prev != done);
                if changed
                    && now - started > 5.0
                    && last_shown.get(&key).is_none_or(|t| now - t > 4.0)
                {
                    last_shown.insert(key, now);
                    let text = objective_text(
                        rt.env.tags.as_deref(),
                        &story,
                        def.quest.texts.get(o.text).to_owned(),
                        key,
                        o,
                    );
                    cinematic.write(CinematicText {
                        head: "tagQuestUpdate".into(),
                        sub: text,
                        sub_args: Vec::new(),
                        kind: 6,
                        front: false,
                    });
                }
            }
        }
    }
}
