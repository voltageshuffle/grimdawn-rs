//! Challenge areas: the sectors of the map's challenge layer (sector layer 13, `ChallengeSectorData`, one
//! `ChallengeArea` record each, [`gd_formats::map::sector::Challenge`]). Stock: the Forgotten Gods sandstorms
//! (`challengelayer_sands_*`) and Fangs of Asterkarn blizzards (`challengelayer_blizzard_*`: only an effect pak),
//! the "Easy" / "Hard" / "Roguelike" secret areas (mutators, scaling, loot weights), the Shattered Realm's treasure
//! room (scaling) and the Asterkarn beaver den (`challengelayer_beaver_01`: also the timed debuff).
//!
//! The area object, `GameEngine::GetChallengeArea`: one per layer id, made and loaded
//! (`ChallengeArea::Load`) on the first lookup that names its record, kept for the session
//! (`ClearAllChallengeAreas` from `ExitPlayingMode`): [`ChallengeAreas`]. Load rolls the area's mutators
//! (`GenerateMutators`, once for `min/maxMutators` + `mutatorPakList`, once for the player list):
//! `IGenerate(min, min(max, list length))` of the shuffled list, each added as an engine mutator of type 2 with
//! the area's id (`AddMutatorOutbound`). The count is `IGenerate` on the engine's `RandomUniformLocked`
//! (`GameEngine.random`: MINSTD a = 48271, seeded once from `_time64` at startup, drawn by ~180 gameplay
//! functions); the shuffle is `std::random_shuffle` on CRT `rand()` (`ChallengeArea_RandomShuffleStrings_rand`).
//! Neither stream is reproducible, so
//! by design the port draws from `gd_game_core::rng` with the same distribution (count uniform, inclusive; uniform shuffle).
//!
//! Who is in which area (`Character.challengeArea`, [`InChallengeArea`]):
//! - the player, every update (`Player::UpdateSelf`): the layer-13 id of its cell in its own
//!   region's level (`SectorLayers::GetTargetId`); none (no region, no entry) leaves the area;
//! - a monster once, when it enters the world (`Monster::InitialUpdate`: its own region's level at its
//!   spawn point; it keeps that area wherever it walks);
//! - containers when they roll their loot (`LootContainer::GenerateFixedItem`, [`challenge_area_at`] in `crate::loot`).
//!
//! What an area does:
//! - the effect pak (`ChallengeArea::ApplyVisualEffects`): only the controlled player, only with a
//!   non-empty `auraCharFxPakName`: `SkillManager::AddCharFxPak` on entering, `RemoveCharFxPak`
//!   on leaving, a character effect pak like a buff's (`crate::casting::attach_buff_fx`, keyed by the
//!   area record);
//! - mutators and scaling ([`AreaRules`], `gd_sim::actor::Rules::with_challenge_area`): `Monster::
//!   GetMonsterMutators` / `Player::GetPlayerMutators` take the engine mutators of type
//!   2 with the area's id (monsters those without `affectsPlayers`, players those with it), except in the Crucible
//!   (type 0) and in the Shattered Realm's dungeon (type 1); `Monster::ContributeGameBalance*` adds the area's
//!   `difficultyAdjustment` pak at the game-balance column;
//! - loot (`gd_sim::items::loot::{challenge_weight_mods, challenge_misc_drop}`): the monster misc drops and the
//!   container drops take the area's weight maps and extra drop;
//! - the timed debuff (`Player::UpdateSelf` + `ChallengeArea::ApplyDebuff`): entering an area
//!   applies it and sets the player's timer (`Player.challengeDebuffTimer`) to `debuffTime` ms; the timer counts
//!   down while above 0 and at
//!   0, still in an area, applies that area's again and restarts. Applying needs the controlled player, a non-empty
//!   `skillBuffCheck` the player does not have active (`Character::IsSkillBuffActive`) and a player that is not
//!   invincible or dead: the `debuff` buff lands like `Character::DebufTarget` (level 1, `debuffTime` long;
//!   `crate::skill_buffs`). Stock: the beaver den's `nerf_buff` every 2 s unless the cauldron's Purity buff
//!   (`energypotion_pure_buff`) runs. PORT: the transfer has no caster (id 0); the port names the player;
//! - the HUD (Grim Dawn.exe `UIPlayerHud::Update`): with `shouldDisplayUI` (default true) the area window shows
//!   and the info icon window lists the area's mutators (`grimdawn::ui::challenge`, `ui/survival.rs`); entering a
//!   displayed area other than the last one announced shows its `nameTag` as a type-0 cinematic text in white
//!   ([`announce`]).
//!
//! - a pet, every update (`Pet::UpdateSelf`: its own region's level at its
//!   position, none without an entry): it takes its area's player mutators (the `Pet` vtable's
//!   `ContributeMutator*` are `Player::`'s, `Pet::GetPetMutators` folded with `Player::GetPlayerMutators`) and
//!   no area scaling (`Pet::ContributeGameBalance*` only add the pet pak). PORT: a pet's region is its owner's
//!   (the port's pets carry none).
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use std::sync::Arc;

use gd_game_core::data::GameData;
use gd_game_core::levels::Levels;
use gd_game_core::player::Player;
use gd_game_core::rules::CombatRules;
use gd_game_core::visuals::BuffFx;
use gd_sim::actor::Rules;

/// The sector layer of the challenge areas.
pub const CHALLENGE_LAYER: usize = 13;

/// The challenge area a character stands in (`Character.challengeArea`): the area's layer id and record. On the
/// player while it is in one, on a monster that entered the world in one.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct InChallengeArea {
    pub id: [i32; 4],
    pub record: String,
}

/// The combat rules of a character in a challenge area (its mutators and scaling, [`ChallengeAreas::rules`]); the
/// profile rebuilds (`player::rebuild_profile`, `monster_skills::rebuild_profiles`) use them instead of
/// [`CombatRules`]. Derived from [`InChallengeArea`] and [`CombatRules`] by this module only.
#[derive(Component, Clone)]
pub struct AreaRules(pub Arc<Rules>);

/// One area object (`ChallengeArea`): its record and the mutator paks `GenerateMutators` rolled.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub record: String,
    pub mutators: Vec<String>,
}

/// The session's area objects by layer id (`GameEngine.challengeAreas`), with the rules derived from them.
#[derive(Resource, Default)]
pub struct ChallengeAreas {
    pub areas: HashMap<[i32; 4], Area>,
    /// The last displayed area whose name was announced (`UIPlayerHud.announcedChallengeArea`).
    pub announced: Option<[i32; 4]>,
    /// Derived rules per (area, keeps the session's mutators), for the [`CombatRules`] they were made from.
    rules: HashMap<([i32; 4], bool), Arc<Rules>>,
    rules_of: Option<Arc<Rules>>,
}

impl ChallengeAreas {
    /// `GameEngine::GetChallengeArea`: the area of `id`, made (mutators rolled) on first use.
    pub fn area(&mut self, db: &gd_formats::vfs::Database, id: [i32; 4], record: &str) -> &Area {
        self.areas.entry(id).or_insert_with(|| Area {
            record: record.to_owned(),
            mutators: roll_mutators(db, record),
        })
    }

    /// The rules of a character in area `id` (made with [`Self::area`]); `keep` (the Crucible, the Shattered
    /// Realm's dungeon): the session's mutators stay.
    pub fn rules(
        &mut self,
        db: &gd_formats::vfs::Database,
        base: &Arc<Rules>,
        id: [i32; 4],
        keep: bool,
    ) -> Option<Arc<Rules>> {
        if !self.rules_of.as_ref().is_some_and(|r| Arc::ptr_eq(r, base)) {
            self.rules.clear();
            self.rules_of = Some(base.clone());
        }
        if let Some(r) = self.rules.get(&(id, keep)) {
            return Some(r.clone());
        }
        let area = self.areas.get(&id)?;
        let adjustment = db
            .get(&area.record)
            .and_then(|r| r.str("difficultyAdjustment"));
        let r = Arc::new(base.with_challenge_area(
            db,
            adjustment,
            (!keep).then_some(&area.mutators[..]),
        ));
        self.rules.insert((id, keep), r.clone());
        Some(r)
    }
}

/// `ChallengeArea::GenerateMutators` for both lists of `record` (module doc).
fn roll_mutators(db: &gd_formats::vfs::Database, record: &str) -> Vec<String> {
    let Some(rec) = db.get(record) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (min, max, list) in [
        ("minMutators", "maxMutators", "mutatorPakList"),
        (
            "minPlayerMutators",
            "maxPlayerMutators",
            "playerMutatorPakList",
        ),
    ] {
        let mut paks: Vec<String> = rec
            .strs(list)
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect();
        let lo = rec.i32(min).unwrap_or(0).max(0) as usize;
        let hi = (rec.i32(max).unwrap_or(0).max(0) as usize).min(paks.len());
        let n = if lo >= hi {
            hi.min(lo)
        } else {
            gd_game_core::rng::usize(lo..=hi)
        };
        gd_game_core::rng::shuffle(&mut paks);
        out.extend(paks.into_iter().take(n));
    }
    out
}

/// The player's debuff timer (`Player.challengeDebuffTimer`, ms; module doc).
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct DebuffTimer(pub i32);

pub struct ChallengeAreasPlugin;

impl Plugin for ChallengeAreasPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChallengeAreas>().add_systems(
            Update,
            (
                track_challenge_area,
                monster_challenge_area,
                pet_challenge_area,
                area_rules,
                pet_profiles,
            )
                .chain()
                .run_if(resource_exists::<Levels>)
                .run_if(resource_exists::<GameData>)
                .run_if(resource_exists::<CombatRules>)
                .before(gd_game_core::player::PlayerSet),
        );
    }
}

/// The challenge area at `pos`: (layer id, record) of the layer-13 entry of the cell in `region`'s level (else
/// whichever loaded level covers `pos`). An entry with an empty record is no area (`GetChallengeArea` makes none).
pub fn challenge_area_at(
    levels: &Levels,
    region: Option<usize>,
    pos: Vec3,
) -> Option<([i32; 4], &str)> {
    let guid = match region.filter(|r| *r != usize::MAX) {
        Some(r) => levels.sector_guid_in(r, pos, CHALLENGE_LAYER)?,
        None => levels.sector_guid(pos, CHALLENGE_LAYER)?,
    };
    levels
        .challenges
        .iter()
        .find(|c| c.base.ints == guid)
        .filter(|c| !c.record.is_empty())
        .map(|c| (guid, c.record.as_str()))
}

/// Whether the session's own mutators win over an area's at `region` (module doc): the Crucible, or a region of
/// the Shattered Realm's dungeon.
fn keeps_mutators(rt: Option<&gd_game_core::story::StoryRuntime>, region: Option<usize>) -> bool {
    let Some(rt) = rt else { return false };
    let s = rt.story.borrow();
    s.game.survival_mode || region.is_some_and(|r| s.endless.in_dungeon(r, false))
}

/// `Player::UpdateSelf`'s challenge-area step (module doc) for the player: the area, its effect pak, the debuff
/// timer and the name announcement.
#[allow(clippy::too_many_arguments)]
fn track_challenge_area(
    mut commands: Commands,
    data: Res<GameData>,
    levels: Res<Levels>,
    time: Res<Time>,
    mut carry: Local<f32>,
    mut areas: ResMut<ChallengeAreas>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut players: Query<
        (
            Entity,
            &Transform,
            Option<&InChallengeArea>,
            Option<&mut DebuffTimer>,
            DebuffTarget,
        ),
        With<Player>,
    >,
    fx: Query<(Entity, &BuffFx)>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
) {
    let region = gd_game_core::riftgates::player_region(rt.as_deref());
    let dt_ms = gd_game_core::session::frame_ms(&mut carry, &time);
    for (player, tf, cur, timer, mut target) in &mut players {
        let now = challenge_area_at(&levels, region, tf.translation);
        let mut left = timer.as_ref().map_or(0, |t| t.0);
        // The timer runs first (`Player.challengeDebuffTimer`), with the area of the last update.
        if left > 0 {
            left -= dt_ms;
            if left <= 0
                && let Some(cur) = cur
            {
                let area = areas.area(&data.db, cur.id, &cur.record).clone();
                apply_debuff(
                    &mut commands,
                    &data,
                    player,
                    tf.translation,
                    &area,
                    &mut target,
                );
                left = debuff_ms(&data, &area.record);
            }
        }
        let changed = now.map(|n| n.0) != cur.map(|c| c.id);
        if changed {
            if let Some(old) = cur {
                for (e, b) in &fx {
                    if b.owner == player && b.skill == old.record {
                        commands.entity(e).try_despawn();
                    }
                }
            }
            match now {
                Some((id, record)) => {
                    debug!("challenge area: {record}");
                    let area = areas.area(&data.db, id, record).clone();
                    commands.entity(player).try_insert(InChallengeArea {
                        id,
                        record: record.to_owned(),
                    });
                    let rec = data.db.get(record);
                    let pak = rec
                        .as_ref()
                        .and_then(|r| r.str("auraCharFxPakName").map(str::to_owned))
                        .filter(|p| !p.is_empty());
                    if let Some(pak) = pak {
                        crate::casting::attach_buff_fx(
                            &mut commands,
                            &data,
                            &pak,
                            player,
                            record,
                            false,
                        );
                    }
                    apply_debuff(
                        &mut commands,
                        &data,
                        player,
                        tf.translation,
                        &area,
                        &mut target,
                    );
                    left = debuff_ms(&data, record);
                }
                None => {
                    commands.entity(player).try_remove::<InChallengeArea>();
                }
            }
        }
        match timer {
            Some(mut t) if t.0 != left => t.0 = left,
            Some(_) => {}
            None if left != 0 => {
                commands.entity(player).try_insert(DebuffTimer(left));
            }
            None => {}
        }
        if let Some((id, record)) = now
            && let Some(tag) = announce(&data, &mut areas, id, record)
        {
            cinematic.write(gd_game_core::story::CinematicText::plain(tag, 0));
        }
    }
}

/// Whether area `record` shows the HUD (`ChallengeArea::GetDisplayUI`: `shouldDisplayUI`, default true).
pub fn displays_ui(data: &GameData, record: &str) -> bool {
    data.db
        .get(record)
        .is_none_or(|r| r.f32("shouldDisplayUI").is_none_or(|v| v != 0.0))
}

/// The HUD update's announcement (`UIPlayerHud::Update`): a displayed area other than the last announced one
/// becomes the last and its `nameTag` is announced (`Engine::ShowCinematicText(name, "", 0, white)`).
pub fn announce(
    data: &GameData,
    areas: &mut ChallengeAreas,
    id: [i32; 4],
    record: &str,
) -> Option<String> {
    if !displays_ui(data, record) || areas.announced == Some(id) {
        return None;
    }
    areas.announced = Some(id);
    data.db
        .get(record)
        .and_then(|r| r.str("nameTag").map(str::to_owned))
}

fn debuff_ms(data: &GameData, record: &str) -> i32 {
    data.db
        .get(record)
        .and_then(|r| r.f32("debuffTime"))
        .map_or(0, |s| (s * 1000.0) as i32)
}

/// What [`apply_debuff`] reads and writes on the player.
type DebuffTarget = (
    Option<&'static mut gd_game_core::skill_buffs::ReceivedBuffs>,
    Option<&'static gd_game_core::player::Loadout>,
    Has<gd_game_core::combat::Dead>,
    Has<crate::combat::Invincible>,
);

/// Whether `buff` runs on the player (`Character::IsSkillBuffActive`: the character's active buffs by record).
/// PORT: the port keeps them in two places: its timed self buffs (the shared
/// [`gd_game_core::skill_buffs::TimedBuffs`] store) and the buffs others put on it.
fn buff_active(
    data: &GameData,
    buff: &str,
    received: Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
    loadout: Option<&gd_game_core::player::Loadout>,
) -> bool {
    let is = |s: &str| {
        s.eq_ignore_ascii_case(buff)
            || data
                .db
                .get(s)
                .and_then(gd_sim::skills::profile_name)
                .is_some_and(|p| p.eq_ignore_ascii_case(buff))
    };
    received.is_some_and(|r| r.has(buff))
        || loadout.is_some_and(|l| l.buffs.list.iter().any(|b| is(&b.0)))
}

/// `ChallengeArea::ApplyDebuff(player, true)` (module doc).
fn apply_debuff(
    commands: &mut Commands,
    data: &GameData,
    player: Entity,
    at: Vec3,
    area: &Area,
    target: &mut bevy::ecs::query::QueryItem<'_, '_, DebuffTarget>,
) {
    let Some(rec) = data.db.get(&area.record) else {
        return;
    };
    let check = rec.str("skillBuffCheck").unwrap_or("");
    let debuff = rec.str("debuff").unwrap_or("");
    let (received, loadout, dead, invincible) = target;
    if check.is_empty()
        || debuff.is_empty()
        || *dead
        || *invincible
        || buff_active(data, check, received.as_deref(), *loadout)
    {
        return;
    }
    let secs = debuff_ms(data, &area.record) as f32 / 1000.0;
    let b = gd_game_core::skill_buffs::ReceivedBuff::new(
        &data.db,
        debuff,
        debuff,
        0,
        player,
        Vec::new(),
        secs,
    );
    let put = match received {
        Some(l) => {
            let put = l.land(b.clone(), |_| true);
            if l.take_changed() {
                commands
                    .entity(player)
                    .try_insert(gd_game_core::player::ProfileDirty);
            }
            put
        }
        None => {
            let mut l = gd_game_core::skill_buffs::ReceivedBuffs::default();
            let put = l.land(b.clone(), |_| true);
            commands
                .entity(player)
                .try_insert((l, gd_game_core::player::ProfileDirty));
            put
        }
    };
    if put.created() {
        crate::skill_buffs::on_created(commands, data, &b, player, at, put);
    }
}

/// `Monster::InitialUpdate`'s challenge-area step: a monster entering the world takes the area of its spawn point
/// in its own region's level (module doc).
fn monster_challenge_area(
    mut commands: Commands,
    data: Res<GameData>,
    levels: Res<Levels>,
    mut areas: ResMut<ChallengeAreas>,
    monsters: Query<
        (Entity, &gd_game_core::monsters::Monster, &Transform),
        Added<gd_game_core::monsters::Monster>,
    >,
) {
    for (e, m, tf) in &monsters {
        let Some((id, record)) = challenge_area_at(&levels, Some(m.slot.0), tf.translation) else {
            continue;
        };
        areas.area(&data.db, id, record);
        commands.entity(e).try_insert(InChallengeArea {
            id,
            record: record.to_owned(),
        });
    }
}

/// `Pet::UpdateSelf`'s challenge-area step (module doc).
fn pet_challenge_area(
    mut commands: Commands,
    data: Res<GameData>,
    levels: Res<Levels>,
    mut areas: ResMut<ChallengeAreas>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    pets: Query<(Entity, &Transform, Option<&InChallengeArea>), With<crate::pets::Pet>>,
) {
    let region = gd_game_core::riftgates::player_region(rt.as_deref());
    for (e, tf, cur) in &pets {
        let now = challenge_area_at(&levels, region, tf.translation);
        if now.map(|n| n.0) == cur.map(|c| c.id) {
            continue;
        }
        match now {
            Some((id, record)) => {
                areas.area(&data.db, id, record);
                commands.entity(e).try_insert(InChallengeArea {
                    id,
                    record: record.to_owned(),
                });
            }
            None => {
                commands.entity(e).try_remove::<InChallengeArea>();
            }
        }
    }
}

/// A pet's [`AreaRules`] came, changed or went (applied by the chain's sync point): its profile is rebuilt
/// (`pets::refresh_profiles`).
fn pet_profiles(
    mut changed: Query<&mut crate::pets::Pet, Changed<AreaRules>>,
    mut gone: RemovedComponents<AreaRules>,
    mut pets: Query<&mut crate::pets::Pet, Without<AreaRules>>,
) {
    for mut p in &mut changed {
        p.mark_profile_dirty();
    }
    for e in gone.read() {
        if let Ok(mut p) = pets.get_mut(e) {
            p.mark_profile_dirty();
        }
    }
}

/// Keeps [`AreaRules`] (and the profile) in step with each character's [`InChallengeArea`] and the
/// [`CombatRules`]: set when the area or the rules change, removed with the area.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn area_rules(
    mut commands: Commands,
    data: Res<GameData>,
    rules: Res<CombatRules>,
    mut areas: ResMut<ChallengeAreas>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut q: Query<(
        Entity,
        Ref<InChallengeArea>,
        Option<&gd_game_core::monsters::Monster>,
        Option<&mut crate::monster_skills::MonsterSkills>,
        Has<Player>,
        Has<AreaRules>,
    )>,
    mut gone: RemovedComponents<InChallengeArea>,
    mut left: Query<
        (
            Has<Player>,
            Option<&mut crate::monster_skills::MonsterSkills>,
        ),
        Without<InChallengeArea>,
    >,
) {
    let player_region = gd_game_core::riftgates::player_region(rt.as_deref());
    for (e, area, monster, ms, is_player, has_rules) in &mut q {
        if !(area.is_changed() || rules.is_changed() || !has_rules) {
            continue;
        }
        // A monster's skills (and profile rebuild) start a frame after it enters the world: wait for them.
        if monster.is_some() && ms.is_none() {
            continue;
        }
        let region = monster.map(|m| m.slot.0).or(player_region);
        let keep = keeps_mutators(rt.as_deref(), region);
        areas.area(&data.db, area.id, &area.record);
        let Some(r) = areas.rules(&data.db, &rules.0, area.id, keep) else {
            continue;
        };
        commands.entity(e).try_insert(AreaRules(r));
        if is_player {
            commands
                .entity(e)
                .try_insert(gd_game_core::player::ProfileDirty);
        }
        if let Some(mut ms) = ms {
            ms.mark_profile_dirty();
        }
    }
    for e in gone.read() {
        let Ok((is_player, ms)) = left.get_mut(e) else {
            continue;
        };
        commands.entity(e).try_remove::<AreaRules>();
        if is_player {
            commands
                .entity(e)
                .try_insert(gd_game_core::player::ProfileDirty);
        }
        if let Some(mut ms) = ms {
            ms.mark_profile_dirty();
        }
    }
}
