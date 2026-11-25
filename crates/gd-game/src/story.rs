//! The story runtime (quests, conversations, Lua scripts) and its effects for gameplay.
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::text::LanguageDef;
use gd_game_core::data::GameData;
use gd_game_core::story::{
    CinematicText, Notify, NpcCast, PLAYER_ID, PlayerSkillCast, StoryActor, StoryEffect,
    StoryProxyChild, StoryRuntime, StorySetUp, StoryStage, objective_text, owner_skill_level,
};
use gd_game_core::story_env::DataEnv;
use gd_story::story::lua::{ScriptEnv, ScriptHost};
use gd_story::story::{Effect, Player as StoryPlayer, Story};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub struct StoryEffectsPlugin;

impl Plugin for StoryEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenShake>()
            .add_message::<StoryEffect>()
            .add_message::<Notify>()
            .add_message::<CinematicText>()
            .add_message::<PlayerSkillCast>()
            .add_systems(PostUpdate, crate::notify::queue_notices);
    }
}

pub struct StoryPlugin;

impl Plugin for StoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (
                    setup_story
                        .run_if(resource_exists::<gd_game_core::session::SessionReady>)
                        .run_if(not(resource_exists::<StorySetUp>)),
                    (
                        sync_items
                            .in_set(StoryStage::Sync)
                            .run_if(resource_exists::<gd_game_core::inventory::Inventory>),
                        mirror_story_monsters.in_set(StoryStage::Sync),
                        count_story_kills.in_set(StoryStage::Sync),
                        publish_attach_coords.in_set(StoryStage::Sync),
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
                        .run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    play_story_animations,
                    cast_story_skills.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    fire_npc_casts,
                    say_bubbles,
                    screen_shake,
                )
                    .after(tick_story),
            ),
        );
    }
}

/// Every monster death goes to the quest kill counters (`gd_story::story::Kill`), credited by
/// [`crate::combat::KillCredit`]; a monster of the Shattered Realm's running dungeon never counts.
/// PORT: the dungeon test is the monster's proxy being one the running dungeon activated; the
/// engine tests the monster's region when it enters the world.
pub fn count_story_kills(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    mut kills: MessageReader<crate::combat::MonsterKilled>,
    credit: crate::combat::KillCredit,
    monsters: Query<&gd_game_core::monsters::Monster>,
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
        story.on_kill(&gd_story::story::Kill {
            by_player: credit.of(k),
            in_dungeon,
            ..gd_story::story::Kill::of_record(&data.db, &k.record)
        });
    }
}

/// A quest action `CastSkill` (`ScriptableAction_CastSkill::Execute`): the action's owner (a
/// conversation's NPC) casts the skill from its own skill list ([`owner_skill_level`]) on the
/// main player, at once, without animation or energy. A skill the owner lacks, or no `Character`
/// owner, does nothing. `Skill_BuffOther::ActivateNow`: the targets are the live non-`Npc`
/// friends of the caster within the buff's `skillTargetRadius`, plus the player; each gets the
/// buff for its `skillActiveDuration`.
/// PORT: the buffs fire at once (no use state or animation on the NPC); the `Skill_PetAttack`
/// type check is not made (no stock NPC lists one).
///
/// A `Skill_AttackRadius` is the NPC's own cast ([`NpcCast`], [`fire_npc_casts`]). PORT: its hits
/// are not dealt (both stock skills deal nothing). [`PlayerSkillCast`] is the player's own cast
/// (the buff shrines), with the same rules.
///
/// A step's actions run in order, synchronously, so a script that destroys the owner leaves a
/// later `CastSkill` without an owner: nothing happens.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn cast_story_skills(
    mut commands: Commands,
    mut events: MessageReader<StoryEffect>,
    data: Res<GameData>,
    rt: Option<NonSend<StoryRuntime>>,
    // Option: factions are optional input to the friendly check.
    factions: Option<Res<crate::factions::PlayerFactions>>,
    actors: Query<(Entity, &StoryActor, &Transform)>,
    mut targets: Query<
        (
            Entity,
            &Transform,
            &gd_game_core::combat::Combatant,
            Option<&mut gd_game_core::skill_buffs::ReceivedBuffs>,
            Has<gd_game_core::player::Player>,
            crate::combat::SideData,
        ),
        (
            Without<gd_game_core::combat::Dead>,
            Without<gd_game_core::world::Breakable>,
        ),
    >,
    mut procs: MessageWriter<gd_game_core::casting::ProcCast>,
    mut player_casts: MessageReader<PlayerSkillCast>,
    mut npc_chars: Query<&mut gd_game_core::character::Character, With<StoryActor>>,
) {
    if events.is_empty() && player_casts.is_empty() {
        return;
    }
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
            && let Some(buff) = rec.str("buffSkillName")
        {
            let brec = data.db.get(buff);
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
                        pack: gd_game_core::monsters::FactionPack::shared(&data, record),
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
                if !is_player && !(near && friend) {
                    continue;
                }
                let b = gd_game_core::skill_buffs::ReceivedBuff::new(
                    &data.db,
                    buff,
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
                        let mut l = gd_game_core::skill_buffs::ReceivedBuffs::default();
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
                    commands
                        .entity(t)
                        .try_insert(gd_game_core::player::ProfileDirty);
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
                gd_game_core::visuals::spawn_fx(
                    &mut commands,
                    &data,
                    &fx,
                    Transform::from_translation(*at),
                );
            }
            continue;
        }
        let kind = gd_game_core::casting::kind_of(rec);
        if kind == gd_game_core::casting::Kind::Unsupported {
            warn!("story: CastSkill {skill} ({}) not supported", rec.class);
            continue;
        }
        info!("story: cast {skill}");
        procs.write(gd_game_core::casting::ProcCast {
            caster: player,
            cast: gd_game_core::casting::Casting {
                skill: skill.clone(),
                level_index,
                kind,
                point: ptf.translation + gd_game_core::character::facing(&ptf) * 2.0,
                target: None,
                hostile: false,
                parent: None,
            },
        });
    }
}

/// An [`NpcCast`] fires at its animation's hit point, else at its "End": the radius effect
/// starts at the NPC.
pub fn fire_npc_casts(
    mut commands: Commands,
    data: Res<GameData>,
    mut events: MessageReader<gd_game_core::character::AnimCallback>,
    casts: Query<(&NpcCast, &Transform)>,
) {
    let mut fired = gd_formats::collections::HashSet::default();
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
            gd_game_core::visuals::spawn_fx(
                &mut commands,
                &data,
                &fx,
                Transform::from_translation(tf.translation),
            );
        }
    }
}

/// `Shrine:IsRestored` reads the devotion shrines restored on this difficulty.
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
    let lang = data.language();
    let env = Rc::new(DataEnv::new(
        data.vfs.clone(),
        data.db.clone(),
        Some(data.tags()),
        lang.clone(),
    ));
    let (defs, dialogs) = (env.quest_defs(), env.dialogs());
    let (nq, nc) = (defs.list.len(), dialogs.defs.len());
    let choice = world.resource::<gd_game_core::session::Choice>().clone();
    let mut player = StoryPlayer::new_character(PLAYER_ID, &choice.name);
    if choice.female {
        player.record = "records/creatures/pc/femalepc01.dbr".into();
    }
    let mut story = Story::new(Arc::new(defs), dialogs, player.clone());
    // The language's dialog paging separators (English ships none: the default).
    story.separators = LanguageDef::load(&data.vfs, &lang).separators;
    // `GameInfo::GetDifficulty` / `GetChallenge` (1 Veteran, 3 Ultimate Veteran), set before the
    // saved quests load.
    if let Some(m) = world.get_resource::<gd_game_core::session::GameMode>() {
        story.game.difficulty = m.0.difficulty.min(2) as u32;
        story.game.challenge = m.0.challenge;
    }
    // The map files load under the save guard (`stash::read_guarded`): one the load cannot read is
    // set aside and never rewritten from the empty runtime.
    world.init_resource::<gd_game_core::stash::SharedFiles>();
    world.resource_scope(|world, mut shared: Mut<gd_game_core::stash::SharedFiles>| {
        let Some(ch) = world.get_resource::<gd_game_core::session::LoadedCharacter>() else {
            return;
        };
        let gdc = &ch.0;
        player.name = gdc.header.name.clone();
        player.level = gdc.bio.level;
        player.experience = gdc.bio.experience;
        player.hardcore = gdc.header.hardcore != 0;
        // Header byte is `GetGender() == 1`, 1 = male.
        if gdc.header.sex == 0 {
            player.record = "records/creatures/pc/femalepc01.dbr".into();
        }
        story.player = player;
        load_save_state(
            &mut story,
            &choice,
            gdc,
            &mut shared,
            world.resource::<gd_game_core::session::SaveDir>(),
        );
    });
    #[cfg(feature = "dev")]
    for t in &world
        .resource::<gd_game_core::hooks::Hooks>()
        .gameplay
        .tokens
    {
        story.player.tokens.insert(t.clone());
    }

    if let Some(ch) = world.get_resource::<gd_game_core::session::LoadedCharacter>() {
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
    // From the gameplay generator, so a seeded game repeats.
    story.rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
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

/// The story's item counts (`Player::GetItemCountInStashes`) follow the player's items: the bags
/// and the held item, not the worn gear, stacks counted; the stash sacks, the transfer stash and
/// the blacksmith's materials go to `stored_items`; incomplete relics and charms of the bags and
/// sacks to `incomplete_items`; the bags' layout to `bags` (`Player.IsSpaceAvailable`).
/// PORT: the held item counts as the inventory's (cursor item placement not traced).
pub fn sync_items(
    inv: Res<gd_game_core::inventory::Inventory>,
    // Option: the stash is absent before `init_stash`.
    stash: Option<Res<gd_game_core::stash::Stash>>,
    data: Res<GameData>,
    rt: Option<NonSend<StoryRuntime>>,
    (mut seen, mut fresh): (Local<u64>, gd_game_core::session::NewSession),
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
    let mut incomplete: HashMap<String, u32> = HashMap::default();
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
    let mut counts: HashMap<String, u32> = HashMap::default();
    for it in inv
        .bags
        .iter()
        .flat_map(|b| b.items.iter().map(|p| &p.item))
        .chain(inv.held.iter())
    {
        add(&mut counts, it);
    }
    let mut stored: HashMap<String, u32> = HashMap::default();
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
    let engine =
        gd_game_core::stack::engine_max_in(inv.db().get(gd_game_core::stack::GAME_ITEM_INFO));
    story.player.bags = inv
        .bags
        .iter()
        .map(|b| gd_formats::inventory::Sack {
            width: b.width,
            height: b.height,
            vertical: b.vertical,
            items: b
                .items
                .iter()
                .map(|p| gd_formats::inventory::Placed {
                    item: gd_story::story::state::BagItem {
                        record: p.item.base_name.clone(),
                        stack_room: gd_game_core::stack::count(&p.item)
                            < gd_game_core::stack::max_of(&inv, &p.item, engine),
                    },
                    x: p.x,
                    y: p.y,
                    size: p.size,
                })
                .collect(),
        })
        .collect();
}

/// The loaded character's `quests.gdd` of every difficulty (the current one last) and
/// `conversations.gdd` of this difficulty in the session's per-map folder, read-only here. A
/// Crucible arena's tokens are the character's survival tokens of this difficulty from
/// `player.gdc`, not the arena file's.
fn load_save_state(
    story: &mut Story,
    choice: &gd_game_core::session::Choice,
    gdc: &gd_formats::save::gdc::Gdc,
    shared: &mut gd_game_core::stash::SharedFiles,
    save_dir: &gd_game_core::session::SaveDir,
) {
    load_map_folder(story, choice, gdc, shared, save_dir);
    if choice.survival.is_some() {
        story.player.tokens = crate::survival::load_tokens(gdc, gdc.info.difficulty());
    }
}

fn load_map_folder(
    story: &mut Story,
    choice: &gd_game_core::session::Choice,
    gdc: &gd_formats::save::gdc::Gdc,
    shared: &mut gd_game_core::stash::SharedFiles,
    save_dir: &gd_game_core::session::SaveDir,
) {
    let Some(root) = choice.save_file(choice.map_folder()) else {
        return;
    };
    let difficulty = gdc.info.difficulty();
    // The write target of each map file: the port's save folder, which may differ from the loaded
    // character's (`root`). The guard sets the target aside / blocks it, never `root`.
    let target_root = choice
        .player_folder(save_dir, &gdc.header.name, gdc.info.is_in_main_quest != 0)
        .join(choice.map_folder());
    let others = (0..gd_story::story::state::DIFFICULTIES).filter(|&d| d != difficulty);
    for d in others.chain([difficulty]) {
        let dir = crate::fog::FogOfWar::dir_name(d);
        let path = root.join(dir).join("quests.gdd");
        let target = target_root.join(dir).join("quests.gdd");
        // A difficulty never played has no file.
        let Some(q) = gd_game_core::stash::read_guarded(shared, &path, &target, |b| {
            gd_formats::save::gdd::Quests::parse(b)
        }) else {
            continue;
        };
        story.load_quests(&q, d);
        info!(
            "story: loaded {} quest states, {} tokens ({})",
            q.quests.len(),
            q.tokens.len(),
            dir
        );
    }
    let dir = crate::fog::FogOfWar::dir_name(difficulty);
    let path = root.join(dir).join("conversations.gdd");
    let target = target_root.join(dir).join("conversations.gdd");
    if let Some(c) = gd_game_core::stash::read_guarded(shared, &path, &target, |b| {
        gd_formats::save::gdd::Conversations::parse(b)
    }) {
        story.load_conversations(&c);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn tick_story(
    rt: Option<NonSendMut<StoryRuntime>>,
    time: Res<Time>,
    mut carry: Local<f32>,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    #[cfg(feature = "dev")] mut fresh: gd_game_core::session::NewSession,
    #[cfg(feature = "dev")] mut lua_ran: Local<bool>,
    players: Query<
        (&Transform, Has<gd_game_core::combat::Dead>),
        With<gd_game_core::player::Player>,
    >,
    // Option: progress and devotion are optional input (the story runs without them).
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    devotion: Option<Res<crate::devotion::DevotionState>>,
    mut out: MessageWriter<StoryEffect>,
    mut cinematic: MessageWriter<CinematicText>,
) {
    #[cfg(feature = "dev")]
    if fresh.begun() {
        *lua_ran = false;
    }
    let Some(mut rt) = rt else { return };
    let rt = &mut *rt;
    publish_shrines(&mut rt.story.borrow_mut(), devotion.as_deref());
    if let Ok((t, dead)) = players.single() {
        let mut s = rt.story.borrow_mut();
        // The scripts mutate the player facts mid-frame: they are restored every tick.
        s.player.position = t.translation.to_array();
        s.player.dead = dead;
        if let Some(p) = &progress {
            s.player.level = p.progress.level.max(1) as u32;
            s.player.experience = p.progress.experience.min(u32::MAX as u64) as u32;
            s.player.money = p.gold.min(u32::MAX as u64) as u32;
        }
        // `TotalDevotion` reads the points ever earned.
        if let Some(d) = &devotion {
            s.player.total_devotion = d.total;
        }
    } else {
        return; // Wait for the player before triggers can fire.
    }
    // `[@secs:]code`, parsed until it ran.
    #[cfg(feature = "dev")]
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
    rt.host
        .update(gd_game_core::session::frame_ms(&mut carry, &time));
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
            // `ShowCinematicText` and Lua `UI.Notify(tag)`: type 9, no subline.
            Effect::CinematicText(tag) | Effect::Notify(tag) => {
                let head = rt.env.localize(tag).unwrap_or(tag.clone());
                cinematic.write(CinematicText::plain(head, 9));
            }
            Effect::QuestTaskStarted { .. }
            | Effect::QuestTaskCompleted { .. }
            | Effect::ObjectiveSatisfied { .. } => {
                info!("story: {e:?}");
                // The quest reward window reads the task starts / completions.
                out.write(StoryEffect(e.clone()));
            }
            // `GameEngine::ScriptEvent`: it only unlocks achievements.
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
            | Effect::TakeItem { .. }
            | Effect::GiveAffixItem { .. }
            | Effect::SetInvincible { .. }
            | Effect::SetSkillsEnabled { .. }
            | Effect::Move { .. }
            | Effect::Attack { .. }
            | Effect::Say { .. }
            | Effect::ScreenShake { .. }
            | Effect::BreakDestructible { .. }
            | Effect::SetAutoClose { .. }
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

/// Quest `ControllerState` actions and `Character.PlayAnimation`: the character plays the
/// `AnimationSet_Type` slot; the latter also puts it in the "QuestPlayAnimation" state, which keeps
/// the monster AI off it unless the call allows quest interruptions.
pub fn play_story_animations(
    mut commands: Commands,
    mut events: MessageReader<StoryEffect>,
    mut actors: Query<(Entity, &StoryActor, &mut gd_game_core::character::Character)>,
) {
    for ev in events.read() {
        let Effect::PlayAnimation {
            id: Some(id),
            anim,
            hold,
            quest,
        } = &ev.0
        else {
            continue;
        };
        let Some(ty) = gd_sim::anim::AnimType::from_index(*anim as usize) else {
            continue;
        };
        for (e, a, mut c) in &mut actors {
            if a.0 == *id
                && c.play_scripted_with(ty, *hold)
                && let Some(allow) = quest
            {
                commands.entity(e).insert(crate::combat::QuestMove {
                    path: Vec::new(),
                    interruptible: *allow,
                    anim: true,
                });
            }
        }
    }
}

/// The armed story monsters as the scripts see them: their position (what a `Monsters` area
/// trigger tests) and, once dead, `Character::IsAlive` false.
pub fn mirror_story_monsters(
    rt: Option<NonSend<StoryRuntime>>,
    // Moved, newly dead or newly armed (`Added<StoryActor>` / `Added<Monster>`, whichever is later).
    monsters: Query<
        (
            &StoryActor,
            &Transform,
            Has<gd_game_core::combat::Dead>,
            &gd_game_core::monsters::Monster,
        ),
        Or<(
            Changed<Transform>,
            Added<gd_game_core::combat::Dead>,
            Added<StoryActor>,
            Added<gd_game_core::monsters::Monster>,
        )>,
    >,
) {
    let Some(rt) = rt else { return };
    let mut s = rt.story.borrow_mut();
    for (&StoryActor(id), tf, dead, m) in &monsters {
        // `Character.GetLevel` reads the placed level the spawn armed.
        s.entities.levels.insert(id, m.level.max(0) as u32);
        if let Some(o) = s.entities.get_mut(id) {
            o.coords.origin = tf.translation.to_array();
            if dead {
                o.alive = false;
            }
        }
    }
}

/// A Shattered Realm essence a script placed is picked up when the player comes within 4 m and
/// used at once, like a dropped one (`PlayerInventoryCtrl::OneShotFilter`).
pub fn pick_up_story_essences(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
) {
    let (Some(rt), Ok(p)) = (rt, players.single()) else {
        return;
    };
    if rt.story.borrow().endless.dungeon.is_none() {
        return;
    }
    let mut s = rt.story.borrow_mut();
    let near: Vec<(u32, String)> = s
        .entities
        .objects
        .values()
        .filter(|o| {
            o.class == gd_story::story::state::ObjClass::Item
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

/// A new story `Character` the player is a foe of fights as a monster: at the script's
/// `Character.Create(record, level)` level (bounty targets), else its record's level at the
/// player's, its AI homed at the placed origin. A story proxy's monster ([`StoryProxyChild`]) or a
/// hooked monster another spawn armed keeps what its spawn gave it.
#[allow(clippy::too_many_arguments)]
pub fn arm_story_characters(
    mut commands: Commands,
    new: Query<
        (Entity, &StoryActor),
        (
            Added<StoryActor>,
            Without<StoryProxyChild>,
            Without<gd_game_core::monsters::Monster>,
        ),
    >,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    rules: Res<gd_game_core::rules::CombatRules>,
    // Option: factions and progress are optional input (friendly check, level 1).
    factions: Option<Res<crate::factions::PlayerFactions>>,
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    levels: Res<gd_game_core::levels::Levels>,
    session: crate::session::Session,
) {
    let Some(rt) = rt else {
        return;
    };
    let story = rt.story.borrow();
    for (e, &StoryActor(id)) in &new {
        let Some(o) = story
            .entities
            .get(id)
            .filter(|o| o.class == gd_story::story::state::ObjClass::Character)
        else {
            continue;
        };
        // Friendly fighters are allies (`combat::Allied`); other friendlies and `Npc`s stand as NPCs.
        let friendly = crate::monsters::is_friendly(&data.db, factions.as_deref(), &o.record);
        if crate::monsters::stands_as_npc(&data.db, factions.as_deref(), &o.record) {
            continue;
        }
        let player = progress.as_ref().map_or(1, |p| p.progress.level);
        // `LuaCharacter::CreateAtLevel`: the origin's area limits (sector layer 9): level≠0 →
        // clamp(level+offset, min, max); level 0 → roll within them; nil origin → the level as
        // given. PORT: only non-zero levels are stored, so a stored-less origin is level 0.
        let limits_at = story
            .entities
            .origins
            .get(&id)
            .and_then(|o| levels.level_limits(None, Vec3::from(*o), session.mode().difficulty));
        let level = match story.entities.levels.get(&id).copied() {
            Some(l) if l != 0 => limits_at.map_or(l as i32, |lim| {
                gd_sim::proxy::area_level(l as i32, Some(lim))
            }),
            _ => {
                let roll = if story.entities.origins.contains_key(&id) {
                    match limits_at {
                        Some((min, max, _)) if min != 0 || max != 0 => {
                            let lo = min.max(1);
                            Some(gd_game_core::rng::u32(lo..=max.max(lo)) as i32)
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                // No script level (or level 0 without limits): [`crate::monsters::placed_char_level`].
                roll.unwrap_or_else(|| {
                    crate::monsters::placed_char_level(&data.db, &o.record, player)
                })
            }
        };
        let Some(p) = gd_sim::actor::monster(
            &data.db,
            &rules.0,
            &o.record,
            level,
            gd_sim::actor::Side::Monster,
        ) else {
            continue;
        };
        let radius = gd_game_core::combat::get_radius(
            data.db.get(&o.record),
            gd_game_core::combat::ACTOR_RADIUS,
        );
        if friendly {
            debug!("story: ally {} (level {level}) {e:?}", o.record);
            commands.entity(e).insert(crate::combat::Allied);
        }
        let mut ai =
            crate::combat::MonsterAi::from_record(&data, &o.record, Vec3::from(o.coords.origin));
        if story.entities.spawn_animation.contains(&id) {
            ai.enable_spawn_animation();
        }
        commands.entity(e).insert((
            gd_game_core::combat::Combatant::new(std::sync::Arc::new(p), radius),
            ai,
            gd_game_core::monsters::Monster {
                record: o.record.clone(),
                level,
                slot: (usize::MAX, id, 0),
            },
        ));
    }
}

/// `Game.UnlockNextDifficulty` → `GameEngine::GameWon`: beating the game on a difficulty at least
/// the unlocked one (and below Ultimate) unlocks both Elite and Ultimate.
pub fn game_won(
    mut commands: Commands,
    mut events: MessageReader<StoryEffect>,
    // Option: this system creates `DifficultyUnlock` and drains the effects without it.
    unlock: Option<ResMut<gd_game_core::session::DifficultyUnlock>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    ready: Option<Res<gd_game_core::session::SessionReady>>,
    mut notify: MessageWriter<CinematicText>,
) {
    let Some(mut unlock) = unlock else {
        if ready.is_some() {
            commands.insert_resource(gd_game_core::session::DifficultyUnlock(
                loaded
                    .as_ref()
                    .map_or(0, |l| gd_game_core::session::unlocked(&l.0)),
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
            notify.write(CinematicText::plain("tagDifficultyUnlocked".to_owned(), 7));
        }
    }
}

/// `Quest2Objective::OnChanged`: when a kill or item count of an objective of a task in progress
/// changes, `tagQuestUpdate` over the objective's line (type 6), at most once per 4 s per
/// objective. The first sight of an objective and the first 5 s of a session only record.
/// PORT: sent here, not by the quest window's collect, so the notice shows with the window closed.
#[allow(clippy::too_many_arguments)]
pub fn quest_updates(
    mut fresh: gd_game_core::session::NewSession,
    rt: Option<NonSend<StoryRuntime>>,
    // Option: the item counts' sources are optional input.
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
    stash: Option<Res<gd_game_core::stash::Stash>>,
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
    // The grace period starts with the session's story (the system runs in the menu too).
    let started = *since.get_or_insert(clock.elapsed_secs());
    let story = rt.story.borrow();
    let log = &story.quests;
    // Nothing an objective counts changes without one of these moving.
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
            if r.state != gd_story::story::quest::STATE_IN_PROGRESS {
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

/// `Character:Say(tag)`: the UI draws the localized tag over the entity until `until` (session
/// seconds).
#[derive(Component, Debug, Clone)]
pub struct SayBubble {
    pub tag: String,
    pub until: f32,
}

/// `Game.ScreenShake(vec, secs, amplitude)`: the render camera decays `secs_left` and offsets by
/// `amplitude`.
#[derive(Resource, Debug, Clone, Default)]
pub struct ScreenShake {
    pub amplitude: f32,
    pub secs_left: f32,
}

/// `LuaCharacter::Say`: a [`SayBubble`] over the entity for ~3 s (no sound, no gameplay).
pub fn say_bubbles(
    mut commands: Commands,
    mut events: MessageReader<StoryEffect>,
    time: Res<Time>,
    actors: Query<(Entity, &StoryActor)>,
    bubbles: Query<(Entity, &SayBubble)>,
) {
    let now = time.elapsed_secs();
    for ev in events.read() {
        if let Effect::Say { id, tag } = &ev.0 {
            for (e, a) in &actors {
                if a.0 == *id {
                    commands.entity(e).insert(SayBubble {
                        tag: tag.clone(),
                        until: now + 3.0,
                    });
                }
            }
        }
    }
    for (e, b) in &bubbles {
        if b.until <= now {
            commands.entity(e).remove::<SayBubble>();
        }
    }
}

/// `Game.ScreenShake` (GameEvent type 7, ms): sets [`ScreenShake`].
pub fn screen_shake(mut events: MessageReader<StoryEffect>, mut shake: ResMut<ScreenShake>) {
    for ev in events.read() {
        if let Effect::ScreenShake { ms, amplitude, .. } = &ev.0 {
            shake.amplitude = *amplitude;
            shake.secs_left = *ms as f32 / 1000.0;
        }
    }
}

/// `Entity:GetAttachedCoords(name)`'s host facts: every frame, the world coords of each story
/// character's mesh attach points (`Actor::GetAttachedCoords`). The names are rebuilt when the body
/// mesh changes or the story's list lost them; a character no longer in the world publishes nothing.
pub fn publish_attach_coords(
    rt: Option<NonSend<StoryRuntime>>,
    chars: Query<(&StoryActor, &gd_game_core::character::Character, &Transform)>,
    (mut seen, mut fresh): (
        Local<HashMap<u32, (usize, bool)>>,
        gd_game_core::session::NewSession,
    ),
) {
    if fresh.begun() {
        seen.clear();
    }
    let Some(rt) = rt else { return };
    let mut s = rt.story.borrow_mut();
    seen.values_mut().for_each(|(_, live)| *live = false);
    for (&StoryActor(id), c, tf) in &chars {
        let points = c.body.attach_points();
        let list = s.entities.attach_coords.entry(id).or_default();
        let ptr = Arc::as_ptr(&c.body) as usize;
        if seen.insert(id, (ptr, true)).map(|(p, _)| p) != Some(ptr) || list.len() != points.len() {
            list.clear();
            list.extend(
                points
                    .iter()
                    .map(|a| (a.name.clone(), gd_story::story::state::Coords::at([0.0; 3]))),
            );
        }
        for (a, (_, out)) in points.iter().zip(list.iter_mut()) {
            let p = a.coords_in(c.posed());
            let axis = |v: [f32; 3]| (tf.rotation * (Vec3::from(v) * tf.scale)).to_array();
            *out = gd_story::story::state::Coords {
                origin: tf.transform_point(Vec3::from(p.origin)).to_array(),
                axes: [axis(p.x), axis(p.y), axis(p.z)],
            };
        }
    }
    seen.retain(|_, (_, live)| *live);
    let seen = &*seen;
    s.entities
        .attach_coords
        .retain(|id, _| seen.contains_key(id));
}
