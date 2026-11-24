//! The story runtime (quests, conversations, Lua scripts) and its effects for
//! gameplay. The game streams story objects in from the map, shows the
//! actors and dialogs, and sends the player's choices.
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
                        publish_item_affixes.in_set(StoryStage::Sync),
                        publish_attach_offsets.in_set(StoryStage::Sync),
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

/// Every monster death goes to the quest kill counters (`gd_story::story::Kill`, types 10 / 16 / 28 / 29): the
/// killer's credit is [`crate::combat::KillCredit`] (the player or his pet, the death handlers' `GetLeader` rule),
/// and a monster of the Shattered Realm's running dungeon (`InEndlessDungeon`) never counts. PORT: the dungeon
/// test is the monster's proxy being one the running dungeon activated (like `progress::kill_experience`); the
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

/// A quest action `CastSkill` (`ScriptableAction_CastSkill`: the Crucible's blessings and power-ups, the
/// devotion purchase, the Shattered Realm's rift openings) activates the skill record at once, without animation or
/// energy, like an item skill.
///
/// `ScriptableAction_CastSkill::Execute`: the action's owner (a conversation's NPC) casts the
/// skill, found in its own skill list ([`owner_skill_level`]), on the main player, at the owner's level. A skill the
/// owner lacks, or an owner that is no `Character` (a quest's events, objectives and tasks run their actions with
/// no owner), does nothing. `Skill_BuffOther::ActivateNow`: the targets are the live non-`Npc`
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
/// Ordering (`ScriptableActionCollection::Execute`): a step's actions run in order,
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
                // The player is the cast's target: always buffed.
                if !is_player && !(near && friend) {
                    continue;
                }
                let b = gd_game_core::skill_buffs::ReceivedBuff::new(
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

/// An [`NpcCast`] fires at its animation's hit point, else at its "End" (as `casting::fire_casts`): the radius
/// effect starts at the NPC.
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
    let lang = data.language();
    let env = Rc::new(DataEnv::new(
        data.vfs.clone(),
        data.db.clone(),
        Some(data.tags()),
        lang.clone(),
    ));
    let (defs, dialogs) = match (env.quest_defs(), env.dialogs()) {
        (Ok(q), Ok(d)) => (q, d),
        (Err(e), _) | (_, Err(e)) => {
            error!("story: {e:#}");
            return;
        }
    };
    let (nq, nc) = (defs.list.len(), dialogs.defs.len());
    let choice = world.resource::<gd_game_core::session::Choice>().clone();
    let mut player = StoryPlayer::new_character(PLAYER_ID, &choice.name);
    if choice.female {
        player.record = "records/creatures/pc/femalepc01.dbr".into();
    }
    let mut story = Story::new(Arc::new(defs), dialogs, player.clone());
    // The language's dialog paging separators (`language.def`; English keeps
    // the default, its archive ships none).
    story.separators = LanguageDef::load(&data.vfs, &lang).separators;
    // The session's `GameInfo` difficulty (0..2) and challenge (1 Veteran on Normal, 3 Ultimate Veteran on Ultimate):
    // `Game.GetGameDifficulty` / `GetGameChallenge` (`LuaGame::GetGameDifficulty` / `LuaGame::GetGameChallenge`
    // read `GameInfo::GetDifficulty` / `GetChallenge`), the quests' current difficulty (set before the saved quests
    // load).
    if let Some(m) = world.get_resource::<gd_game_core::session::GameMode>() {
        story.game.difficulty = m.0.difficulty.min(2) as u32;
        story.game.challenge = m.0.challenge;
    }
    // The map files load under the save guard (`stash::read_guarded`): one the load cannot read is set
    // aside and never rewritten from the empty runtime. `SharedFiles` is keyed by the write target, so the
    // guard needs the session's save folder beside the loaded character.
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
        // Header byte is `GetGender() == 1`, 1 = male
        // (`Character::Load`: "Male" → 1, "Female" → 2).
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
    // `GD_TOKENS` (hooks.rs).
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
    // The session's random stream (`Story::rng`): from the gameplay generator, so a seeded game repeats.
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

/// The story's item counts (`HasItem` conditions, Lua `HasItem`, both `Player::GetItemCountInStashes`, see
/// [`gd_story::story::Player::item_count_in_stashes`]) follow the player's items: the bags and the held item, not
/// the worn gear (`Inventory::GetItemCount` skips what `EquipmentCtrl::GetEquippedLocation`
/// finds), stacks counted; the stash sacks, the transfer stash and the blacksmith's materials go to
/// `stored_items`; the incomplete relics and charms of the bags and sacks (not the materials) to
/// `incomplete_items`. PORT: the held item counts as the inventory's (the engine's cursor item placement is not
/// traced). The bags themselves go to `bags` (`Player.IsSpaceAvailable`): each item's record, cell and footprint,
/// and whether its stack has room for one more (`gd_game_core::stack::max_of`).
pub fn sync_items(
    inv: Res<gd_game_core::inventory::Inventory>,
    // Option: the stash is optional input (before `init_stash`).
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

/// The loaded character's `quests.gdd` of every difficulty (the current one last: `gd_story::story::save`) and
/// `conversations.gdd` of this difficulty in the session's per-map folder
/// (`Choice::map_folder`: the campaign's or the Crucible arena's), read-only here; `save.rs` writes them back.
/// A Crucible arena without a file starts with no quest state; its tokens are the character's survival tokens of
/// this difficulty from `player.gdc` (shared by the arenas, [`crate::survival::load_tokens`]), not the arena file's.
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
    // The write target of each map file (`save.rs`): the port's save folder, which may differ from the
    // loaded character's (`root`). The guard sets the target aside / blocks it, never `root`.
    let target_root = choice
        .player_folder(save_dir, &gdc.header.name, gdc.info.is_in_main_quest != 0)
        .join(choice.map_folder());
    let others = (0..gd_story::story::state::DIFFICULTIES).filter(|&d| d != difficulty);
    for d in others.chain([difficulty]) {
        let dir = crate::fog::FogOfWar::dir_name(d);
        let path = root.join(dir).join("quests.gdd");
        let target = target_root.join(dir).join("quests.gdd");
        // A difficulty never played has no file (`read_guarded`: absent → a new empty one, no guard).
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
    hooks: Res<gd_game_core::hooks::Hooks>,
    (mut fresh, mut lua_ran): (gd_game_core::session::NewSession, Local<bool>),
    players: Query<
        (Ref<Transform>, Has<gd_game_core::combat::Dead>),
        With<gd_game_core::player::Player>,
    >,
    // Option: progress and devotion are optional input (the story runs without them).
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
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
        // PERF: the position only moves when the Transform does. The scalar facts stay unconditional:
        // the scripts mutate them mid-frame (`gd_story::story`) and the game's player is their source of
        // truth, restored every tick; `dead` is a bool that must also track the respawn (Dead removed).
        if t.is_changed() {
            s.player.position = t.translation.to_array();
        }
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
            // The scripts' `ShowCinematicText` action: type 9 ("quest-script texts").
            // Lua `UI.Notify(tag)` (`LuaUi::Notify` → `GameEngine::QuestCommandUiNotify`): the same, no
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

/// Quest `ControllerState` actions: the conversation's NPC plays the
/// `AnimationSet_Type` slot ([`gd_game_core::character::Character::play_scripted`]).
pub fn play_story_animations(
    mut events: MessageReader<StoryEffect>,
    mut actors: Query<(&StoryActor, &mut gd_game_core::character::Character)>,
) {
    for ev in events.read() {
        let Effect::PlayAnimation {
            id: Some(id),
            anim,
            hold,
            flag,
        } = &ev.0
        else {
            continue;
        };
        let Some(ty) = gd_sim::anim::AnimType::from_index(*anim as usize) else {
            continue;
        };
        for (a, mut c) in &mut actors {
            if a.0 == *id {
                c.play_scripted_with(ty, *hold, *flag);
            }
        }
    }
}

/// The armed story monsters as the scripts see them: their position (`Entity::GetCoords` of the live character:
/// the coords a `Monsters` area trigger tests, `gd_story::story::trigger`) and, once dead, `Character::IsAlive` false
/// (the Shattered Realm's Loghorrean room counts its three bosses dead by `Character.IsAlive`).
pub fn mirror_story_monsters(
    rt: Option<NonSend<StoryRuntime>>,
    // PERF: only moved / newly dead / newly armed actors are rewritten; an idle monster's coords and
    // alive flag do not change. `Added<StoryActor>` covers the frame it is armed, before it first moves.
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
        )>,
    >,
) {
    let Some(rt) = rt else { return };
    let mut s = rt.story.borrow_mut();
    for (&StoryActor(id), tf, dead, m) in &monsters {
        // `Character.GetLevel` reads the placed level (the engine's `charLevel` dword,
        // `ExperienceLevelControl`): the host mirrors what the spawn armed.
        s.entities.levels.insert(id, m.level.max(0) as u32);
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
/// one (`PlayerInventoryCtrl::OneShotFilter`, the 4 m auto-pickup;
/// `endless::progress` applies its progress and soul charge).
pub fn pick_up_story_essences(
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
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

/// A new story `Character` the player is a foe of
/// ([`crate::monsters::is_friendly`]) fights as a monster: at the script's
/// `Character.Create(record, level)` level (bounty targets), else its record's
/// level at the player's, its AI homed at the object's placed origin. A story
/// proxy's monster ([`StoryProxyChild`]) or a hooked monster another spawn armed (`crate::story_world::
/// hooked_characters`: it already has its [`gd_game_core::monsters::Monster`]) keeps what its spawn gave it.
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
        // A friendly creature of class Monster with a controller that fights (a Crucible defense turret) is an
        // ally of the player (`combat::Allied`); the other friendly ones and every `Npc` stand as NPCs
        // (`monsters::stands_as_npc`).
        let friendly = crate::monsters::is_friendly(&data.db, factions.as_deref(), &o.record);
        if crate::monsters::stands_as_npc(&data.db, factions.as_deref(), &o.record) {
            continue;
        }
        let player = progress.as_ref().map_or(1, |p| p.progress.level);
        // `Character.Create(record, level, origin)` (exe `0x140051540`): the origin's area limits
        // (`GetLevelLimits`, sector layer 9): level≠0 → clamp(level+offset, min, max); level 0 →
        // roll within them; nil origin → (level, level, 0), the level as given. PORT: the binding
        // stores only non-zero levels, so an origin with no stored level is the 3-arg form's level 0.
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
                // No script level (or level 0 without limits): `Character::Load`'s own level at the
                // player's (`monsters::placed_char_level`).
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
        // `Character.Create` enables the spawn animation (`Entities::spawn_animation`).
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

/// `Game.UnlockNextDifficulty` → `GameEngine::GameWon`: beating the game on a
/// difficulty at least the unlocked one (and below Ultimate) unlocks both Elite
/// and Ultimate (see `grimdawn/src/menu.rs`).
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
            // Cinematic text type 7 (difficulty unlocked).
            notify.write(CinematicText::plain("tagDifficultyUnlocked".to_owned(), 7));
        }
    }
}

/// `Quest2Objective::OnChanged`: when a kill or item
/// count of an objective of a task in progress changes, `tagQuestUpdate` over the objective's line (type 6),
/// at most once per 4 s per objective. The first sight of an objective only records its count, and the first
/// 5 s of a session only record (the counts settle while the inventory arrives after the story).
/// PORT: sent here, not by the quest window's collect, so the notice shows with the window closed; the window
/// walks the quests in repository order, this in definition order (only same-frame notice order differs).
#[allow(clippy::too_many_arguments)]
pub fn quest_updates(
    mut fresh: gd_game_core::session::NewSession,
    rt: Option<NonSend<StoryRuntime>>,
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

/// `Character:Say(tag)` (the engine's speech-bubble UI vcall): presentational. The UI draws the
/// localized tag over the entity until `until` (session seconds); headless nothing reads it.
#[derive(Component, Debug, Clone)]
pub struct SayBubble {
    pub tag: String,
    pub until: f32,
}

/// `Game.ScreenShake(vec, secs, amplitude)` (engine GameEvent type 7): presentational camera shake.
/// The render camera decays `secs_left` and offsets by `amplitude`; headless nothing reads it.
#[derive(Resource, Debug, Clone, Default)]
pub struct ScreenShake {
    pub amplitude: f32,
    pub secs_left: f32,
}

/// `Character:Say(tag)` (exe `0x1400513d0`: the localized tag goes to the UI speech-bubble vcall; no
/// sound, no gameplay): a [`SayBubble`] over the entity for ~3 s. The expiry is the host's (the UI only
/// reads); headless nothing draws it.
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

/// `Game.ScreenShake(vec, secs, amplitude)` (engine GameEvent type 7, ms): sets the presentational
/// [`ScreenShake`] the render camera reads; headless nothing does.
pub fn screen_shake(mut events: MessageReader<StoryEffect>, mut shake: ResMut<ScreenShake>) {
    for ev in events.read() {
        if let Effect::ScreenShake { ms, amplitude, .. } = &ev.0 {
            shake.amplitude = *amplitude;
            shake.secs_left = *ms as f32 / 1000.0;
        }
    }
}

/// `Item:HasAffix`'s host bridge (LUA-13): the live affixes of the inventory items the story knows.
/// PORT: the port's items carry no object ids (the engine's `Item` objects do), so a story item object
/// is bridged to the first inventory instance of its record (bags, worn gear, the stash sacks); an
/// object with no instance publishes nothing (`HasAffix` false).
pub fn publish_item_affixes(
    rt: Option<NonSend<StoryRuntime>>,
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
    stash: Option<Res<gd_game_core::stash::Stash>>,
) {
    let (Some(rt), Some(inv)) = (rt, inv) else {
        return;
    };
    let mut s = rt.story.borrow_mut();
    let items: Vec<(u32, String)> = s
        .entities
        .objects
        .values()
        .filter(|o| o.class == gd_story::story::state::ObjClass::Item)
        .map(|o| (o.id, o.record.clone()))
        .collect();
    for (id, record) in items {
        let bags = inv
            .bags
            .iter()
            .flat_map(|b| b.items.iter().map(|p| &p.item));
        let worn = inv.equipped.iter().flatten();
        let stored = stash
            .iter()
            .flat_map(|st| st.pages.iter().chain(&st.transfer))
            .flat_map(|b| b.items.iter().map(|p| &p.item));
        let affixes = bags
            .chain(worn)
            .chain(stored)
            .find(|it| it.base_name.eq_ignore_ascii_case(&record))
            .map(|it| (it.prefix_name.clone(), it.suffix_name.clone()));
        match affixes {
            Some(a) => {
                s.entities.item_affixes.insert(id, a);
            }
            None => {
                s.entities.item_affixes.remove(&id);
            }
        }
    }
}

/// `Entity:GetAttachedCoords(name)`'s host facts (LUA-18): the mesh attach points of the story
/// characters (`gd_formats::msh::Mesh::attach_points`), published as local offsets the binding adds to
/// the entity coords. PORT: translation only — the engine applies the attach point's full transform
/// (its parent bone's animated frame × the point); the port publishes the point's mesh-space origin
/// (no bone animation, no attach rotation), keyed lowercase (the engine's name lookup is
/// case-insensitive). Republished when the body mesh changes (a form); objects without a mesh keep
/// the binding's entity-coords fallback.
pub fn publish_attach_offsets(
    rt: Option<NonSend<StoryRuntime>>,
    chars: Query<(&StoryActor, &gd_game_core::character::Character)>,
    mut seen: Local<HashMap<u32, usize>>,
) {
    let Some(rt) = rt else { return };
    let mut s = rt.story.borrow_mut();
    for (&StoryActor(id), c) in &chars {
        let ptr = Arc::as_ptr(&c.body) as usize;
        if seen.get(&id) == Some(&ptr) {
            continue;
        }
        seen.insert(id, ptr);
        s.entities.attach_offsets.retain(|(oid, _), _| *oid != id);
        for a in c.body.attach_points() {
            s.entities
                .attach_offsets
                .insert((id, a.name.to_ascii_lowercase()), a.coords.origin);
        }
    }
    seen.retain(|id, _| s.entities.get(*id).is_some());
}
