//! Loot chests: opening one (in reach, unlocked) drops its loot at the container level once a
//! session; story locks; culling with the level. A click is the player's interact order, which
//! [`open_chests`] takes once in reach (player radius + [`Chest::radius`] + `usableRange`, 1 m
//! without one; +1 m at the walk's end, 3D).
//!
//! `FixedItemContainer::Open`: plays the record's `openSound` at the chest, starts
//! `openAnimation`, adds `openFxPakName` to the world at the chest (not attached), then the
//! loot is thrown one item at a time from `FixedItem::GetLootDropCoords` with `actorRadius`.
//!
//! The radius lock (`FixedItemContainer::CheckRadiusClear`): an unopened container with
//! `LockedRadius` (> 0.0001) sets its `locked` flag every 500 ms: locked while a live `Monster`
//! within that radius has a classification at or above its `LockedClassification` (`Common` 0,
//! `Champion` 1, `Hero` 2, `Boss` 3, `Quest` 4, anything else 999: never locks). Boss chests
//! unlock so once their boss is dead. PORT: the monsters' centres, one timer for every chest.
//!
//! Food crates (`FixedItemShrine`) are used like chests ([`Chest::shrine`]):
//! - `FixedItemShrineController::TouchedByActor` (idle only) goes active for `shrineActiveTime`,
//!   remembering the toucher; `Update` samples at once and every `shrineSampleTime` while active
//!   the toucher's friends within `shrineRadius` of the crate
//!   (`GameEngine::GetTargetsInRadius(…, IsFriend)` → `FixedItem::GiveBonusToTargets` →
//!   `Character::TakeBonus` of the crate's `bonus*` fields), the player's pets included. Both
//!   timers count down together, so the stock 1 s / 1 s gives one bonus per use. When
//!   `shrineActiveTime` runs out the crate goes dormant for `shrineDormantTime`, then back to
//!   idle unless `oneTimeUse` (every stock crate is one-time).
//! - Use: `idleToActiveSound` at the crate and `ActiveOneShotEffect`; the idle effects stop
//!   emitting and stay in the world. The stock crates have no active / dormant animations.
//!
//! PORT: [`Chest::phase`] counts the timers down by the frame delta (the engine's whole-ms
//! countdown); the friendly sample takes live characters' centres (the radius test of
//! `radius_locks`); no loot (the stock crates have no loot table).
use crate::movement::MovePath;
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::player::Player;
use gd_game_core::visuals::PlaySound;
use gd_game_core::world::{ChestKey, ChestStates};
use gd_rules::Rng;

#[derive(Component)]
pub struct Chest {
    pub record: String,
    pub key: ChestKey,
    pub opened: bool,
    /// Level of the proxy that placed it (0: none).
    pub proxy_level: i32,
    /// `Actor::GetRadius`: the use range's and the hover pick's.
    pub radius: f32,
    /// The record's `locked`; story `SetLocked` changes it.
    pub locked: bool,
    /// `LockedRadius` and the `LockedClassification` level.
    pub lock_radius: Option<(f32, u32)>,
    /// A `FixedItemShrine` (a food crate): its use gives a bonus, not loot.
    pub shrine: bool,
    /// The record's `oneTimeUse`: a used one-time crate never returns to idle.
    pub one_time: bool,
    /// `FixedItemShrineController`'s state (module docs): idle, active, dormant or spent.
    pub phase: ShrinePhase,
}

/// `FixedItemShrineController`'s state; the timers are seconds left. A one-time crate ends
/// `Spent` (the port's opened chest); without `oneTimeUse` the dormant crate returns to idle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ShrinePhase {
    #[default]
    Idle,
    /// (active time left, time to the next sample).
    Active(f32, f32),
    Dormant(f32),
    Spent,
}

impl Chest {
    /// `FixedItemContainer::IsOfInterest`: closed. A locked chest is still of interest (a click
    /// tells it is locked).
    pub fn is_of_interest(&self) -> bool {
        !self.opened
    }
}

/// A story chest was opened (the story runs its `onOpen` hooks).
#[derive(Message, Clone, Debug)]
pub struct ChestOpened(pub u32);

/// A path to beside a chest: chests usually stand on cells the nav data blocks, so the goal is
/// the chest, else the first reachable point on a ring around it.
pub fn path_near(
    nav: &gd_sim::nav::NavWorld,
    scratch: &mut gd_sim::nav::PathScratch,
    from: Vec3,
    at: Vec3,
    radius: f32,
) -> Option<Vec<Vec3>> {
    let ring = (0..8).map(|i| {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        at + Vec3::new(a.cos(), 0.0, a.sin()) * (radius + 0.7)
    });
    std::iter::once(at)
        .chain(ring)
        .find_map(|g| {
            nav.find_path(
                scratch,
                from.to_array(),
                g.to_array(),
                crate::nav::PATH_BUDGET_LONG,
            )
        })
        .map(|p| p.into_iter().skip(1).map(Vec3::from).collect())
}

#[allow(clippy::too_many_arguments)]
pub fn open_chests(
    mut commands: Commands,
    mut states: ResMut<ChestStates>,
    mut chests: Query<(&GlobalTransform, &mut Chest)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut crate::player_orders::InteractTarget,
            Option<&gd_game_core::combat::Combatant>,
            &gd_game_core::character::Character,
        ),
        With<Player>,
    >,
    // Option: level 1 when absent.
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    world: Res<gd_game_core::levels::Levels>,
    nav: Res<Nav>,
    data: Res<gd_game_core::data::GameData>,
    codex: Res<crate::lore::LoreCodex>,
    (mut opened_out, mut stats): (
        MessageWriter<ChestOpened>,
        MessageWriter<crate::play_stats::PlayStat>,
    ),
    mut locked_out: MessageWriter<LockedUse>,
    mut sound: MessageWriter<PlaySound>,
    mut loot: crate::loot::ContainerLoot,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
) {
    let Ok((ptf, mut path, mut interact, pcb, ch)) = players.single_mut() else {
        return;
    };
    let Some(e) = interact.0 else { return };
    let Ok((gt, mut chest)) = chests.get_mut(e) else {
        return;
    };
    let Some(rec) = data.db.get(&chest.record) else {
        return;
    };
    let at = gt.translation();
    // +1 m only once the walk has ended.
    let range = gd_game_core::player_orders::usable_range(
        pcb.map_or(0.5, |c| c.radius),
        chest.radius,
        gd_game_core::combat::fixed_usable_range(Some(rec)),
        path.points.is_empty(),
    );
    if chest.opened || ptf.translation.distance(at) > range {
        return;
    }
    if chest.locked {
        interact.0 = None;
        path.points.clear();
        locked_out.write(LockedUse {
            record: chest.record.clone(),
            at,
        });
        // `Player::PlayLockedChestVox`.
        if let Some(pak) =
            crate::char_sounds::locked_vox(&data, ch, crate::char_sounds::LockedKind::Chest)
        {
            sound.write(PlaySound {
                pak,
                at: Some(ptf.translation),
            });
        }
        return;
    }
    interact.0 = None;
    path.points.clear();
    if chest.shrine {
        // `FixedItemShrineController::TouchedByActor`: idle only; the first sample runs in
        // `shrine_timers` this frame (the controller's sample timer starts at 0).
        let active = rec.f32("shrineActiveTime").unwrap_or(0.0);
        chest.phase = if chest.one_time && active <= 0.0 {
            ShrinePhase::Spent
        } else {
            ShrinePhase::Active(active, 0.0)
        };
        chest.opened = true;
        states.opened.insert(chest.key);
        stats.write(crate::play_stats::PlayStat::ChestOpened(
            chest.record.clone(),
        ));
        if let Some(pak) = rec.str("idleToActiveSound") {
            sound.write(PlaySound {
                pak: pak.to_owned(),
                at: Some(at),
            });
        }
        if let Some(fx) = rec.str("ActiveOneShotEffect") {
            let (_, rot, pos) = gt.to_scale_rotation_translation();
            gd_game_core::visuals::spawn_fx(
                &mut commands,
                &data,
                fx,
                Transform::from_translation(pos).with_rotation(rot),
            );
        }
        return;
    }
    chest.opened = true;
    states.opened.insert(chest.key);
    info!("chest {} opened at {at:.1}", chest.record);
    stats.write(crate::play_stats::PlayStat::ChestOpened(
        chest.record.clone(),
    ));
    if let Some(pak) = rec.str("openSound").filter(|s| !s.is_empty()) {
        sound.write(PlaySound {
            pak: pak.to_owned(),
            at: Some(at),
        });
    }
    if let Some(fx) = rec.str("openFxPakName").filter(|s| !s.is_empty()) {
        let (_, rot, pos) = gt.to_scale_rotation_translation();
        gd_game_core::visuals::spawn_fx(
            &mut commands,
            &data,
            fx,
            Transform::from_translation(pos).with_rotation(rot),
        );
    }
    let player_level = progress.as_ref().map_or(1, |p| p.progress.level);
    let difficulty = loot.session.mode().difficulty;
    let limits = world.level_limits(
        match chest.key {
            ChestKey::Level(l, _) | ChestKey::Accessory(l, _, _) => Some(l),
            ChestKey::Story(_) => None,
        },
        at,
        difficulty,
    );
    let level = gd_sim::items::loot::container_level(
        rec,
        player_level,
        difficulty,
        limits,
        chest.proxy_level,
    );
    let idb = &data.item_db();
    let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    let drops = loot.drops(
        idb,
        &chest.record,
        level,
        player_level as u32,
        Some(at),
        &mut rng,
    );
    let gold = rec
        .str("goldGenerator")
        .map(|g| {
            gd_sim::progress::gold_drop(
                &data.db,
                g,
                rec.f32("goldGeneratorChance").unwrap_or(0.0),
                level as i32,
                &mut rng,
            )
        })
        .unwrap_or_default();
    info!(
        "opened {} (level {level}): {} items, gold {gold:?}",
        chest.record.rsplit('/').next().unwrap_or(""),
        drops.len()
    );
    // The chest's lore note; one already read is seeded but not dropped.
    let note = rec
        .str("perPartyMemberDropItemName")
        .filter(|n| !n.is_empty())
        .map(|n| gd_formats::save::item::Item {
            base_name: n.to_owned(),
            stack_count: 1,
            seed: rng.next().max(1),
            ..Default::default()
        })
        .filter(|it| !codex.knows(&it.base_name));
    let body = rec.str("mesh").and_then(|m| bodies.get(&data, m));
    let drop = crate::loot::fixed_drop_point(&nav, at, gt.rotation(), body.as_deref());
    crate::loot::drop_loot_by(
        &mut commands,
        idb,
        drop,
        drops.iter().map(|d| d.to_item()).chain(note),
        &gold,
        crate::loot::Dropper::one_by_one(
            rec.f32("actorRadius")
                .unwrap_or(gd_game_core::combat::ACTOR_RADIUS),
        ),
    );
    if let ChestKey::Story(id) = chest.key {
        opened_out.write(ChestOpened(id));
    }
}

#[cfg(feature = "dev")]
pub fn test_open(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut timer: Local<f32>,
    time: Res<Time>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    nav: Res<Nav>,
    chests: Query<(Entity, &GlobalTransform, &Chest)>,
    mut players: Query<
        (
            &Transform,
            &mut MovePath,
            &mut crate::player_orders::InteractTarget,
        ),
        With<Player>,
    >,
) {
    if !hooks.gameplay.open_chests {
        return;
    }
    *timer -= time.delta_secs();
    if *timer > 0.0 || players.single().is_ok_and(|(_, _, t)| t.0.is_some()) {
        return;
    }
    *timer = 2.0;
    let Ok((tf, mut path, mut interact)) = players.single_mut() else {
        return;
    };
    let best = chests
        .iter()
        .filter(|(_, _, c)| !c.opened)
        .map(|(e, gt, _)| {
            (
                e,
                gt.translation(),
                gt.translation().distance(tf.translation),
            )
        })
        .filter(|c| c.2 < hooks.gameplay.open_chests_range)
        .min_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((e, at, _)) = best
        && let Some(p) = path_near(&nav.0, &mut scratch, tf.translation, at, 0.4)
    {
        interact.0 = Some(e);
        path.points = p;
    }
}

/// Script `SetLocked` on story chests (remembered for chests spawned later).
pub fn story_locks(
    mut fresh: gd_game_core::session::NewSession,
    mut events: MessageReader<gd_game_core::story::StoryEffect>,
    mut chests: Query<&mut Chest>,
    mut locks: Local<gd_formats::collections::HashMap<u32, bool>>,
) {
    if fresh.begun() {
        *locks = Default::default();
    }
    for ev in events.read() {
        if let gd_story::story::Effect::SetLocked { id, locked } = ev.0 {
            locks.insert(id, locked);
        }
    }
    for mut c in &mut chests {
        if let ChestKey::Story(id) = c.key
            && let Some(&l) = locks.get(&id)
            && c.locked != l
        {
            c.locked = l;
        }
    }
}

pub fn radius_locks(
    time: Res<Time>,
    mut timer: Local<f32>,
    data: Res<gd_game_core::data::GameData>,
    mut chests: Query<(&GlobalTransform, &mut Chest)>,
    monsters: Query<
        (&GlobalTransform, &gd_game_core::monsters::Monster),
        Without<gd_game_core::combat::Dead>,
    >,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.5;
    for (gt, mut chest) in &mut chests {
        let Some((radius, class)) = chest.lock_radius.filter(|_| !chest.opened) else {
            continue;
        };
        let at = gt.translation();
        let locked = monsters.iter().any(|(m, monster)| {
            m.translation().distance(at) <= radius
                && data.db.get(&monster.record).is_some_and(|r| {
                    let c = r.str("monsterClassification").unwrap_or("");
                    gd_story::story::quest::monster_classification(c) >= class
                })
        });
        if chest.locked != locked {
            chest.locked = locked;
        }
    }
}

/// `FixedItemShrineController::Update`: both timers count down together; while active, every
/// `shrineSampleTime` (the first at once) the toucher's friends within `shrineRadius` take the
/// crate's bonus (`FixedItem::GiveBonusToTargets` → `Character::TakeBonus`), the player's pets
/// and allied characters included. Active time out: dormant for `shrineDormantTime`, then back
/// to idle unless `oneTimeUse`. A one-time crate with no active time is spent at its use.
#[allow(clippy::too_many_arguments)]
pub fn shrine_timers(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    mut states: ResMut<ChestStates>,
    mut chests: Query<(&GlobalTransform, &mut Chest)>,
    players: Query<Entity, With<Player>>,
    friends: Query<
        (
            Entity,
            &GlobalTransform,
            &gd_game_core::combat::Combatant,
            crate::combat::VictimSide,
        ),
        Without<gd_game_core::combat::Dead>,
    >,
    mut constitutions: Query<&mut crate::constitution::Constitution>,
) {
    let dt = time.delta_secs();
    let Ok(player) = players.single() else { return };
    for (gt, mut chest) in &mut chests {
        if !chest.shrine {
            continue;
        }
        let Some(rec) = data.db.get(&chest.record) else {
            continue;
        };
        let sample = rec.f32("shrineSampleTime").unwrap_or(0.0);
        // Both timers count down; the sample runs before the active time's end is taken
        // (the stock 1 s / 1 s gives one bonus per use), then the dormant time starts.
        let phase = match chest.phase {
            ShrinePhase::Active(mut left, mut next) => {
                next -= dt;
                left -= dt;
                if next <= 0.0 {
                    next += sample;
                    shrine_sample(
                        &mut commands,
                        rec,
                        gt.translation(),
                        player,
                        &friends,
                        &mut constitutions,
                    );
                }
                if left <= 0.0 {
                    let dormant = rec.f32("shrineDormantTime").unwrap_or(0.0);
                    if chest.one_time && dormant <= 0.0 {
                        ShrinePhase::Spent
                    } else {
                        ShrinePhase::Dormant(dormant)
                    }
                } else {
                    ShrinePhase::Active(left, next)
                }
            }
            ShrinePhase::Dormant(mut left) => {
                left -= dt;
                if left > 0.0 {
                    ShrinePhase::Dormant(left)
                } else if chest.one_time {
                    ShrinePhase::Spent
                } else {
                    // Back to idle: usable again (a level respawn finds it closed), its idle
                    // effect restarts.
                    chest.opened = false;
                    states.opened.remove(&chest.key);
                    ShrinePhase::Idle
                }
            }
            _ => continue,
        };
        chest.phase = phase;
    }
}

/// One sample: the toucher's friends whose centre is within `shrineRadius`
/// (`GameEngine::GetTargetsInRadius(…, IsFriend)`, hiding characters left out), each taking the
/// crate's `bonus*` fields: life / mana like a skill's bonus ([`crate::casting::Bonus`]) and
/// constitution into the player's pool ([`crate::constitution::Constitution`]).
fn shrine_sample(
    commands: &mut Commands,
    rec: &gd_formats::arz::Record,
    at: Vec3,
    player: Entity,
    friends: &Query<
        (
            Entity,
            &GlobalTransform,
            &gd_game_core::combat::Combatant,
            crate::combat::VictimSide,
        ),
        Without<gd_game_core::combat::Dead>,
    >,
    constitutions: &mut Query<&mut crate::constitution::Constitution>,
) {
    let radius = rec.f32("shrineRadius").unwrap_or(0.0);
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    let bonus = crate::casting::Bonus {
        life: f("bonusLifePoints"),
        life_pct: f("bonusLifePercent"),
        mana: f("bonusManaPoints"),
        mana_pct: f("bonusManaPercent"),
        ..default()
    };
    let (pct, pts) = (f("bonusConstitutionPercent"), f("bonusConstitutionPoints"));
    for (e, tf, cb, side) in friends {
        if side.hiding()
            || !side.character()
            || crate::combat::Side::Player.foe(side.side.side_ref())
            || tf.translation().distance(at) > radius
        {
            continue;
        }
        bonus.queue(commands, e);
        if e == player
            && let Ok(mut c) = constitutions.get_mut(e)
        {
            c.take_bonus(cb.profile.sheet.constitution, pct, pts);
            info!(
                "food crate {}: constitution {:.0}",
                rec.name.rsplit('/').next().unwrap_or(""),
                c.current
            );
        }
    }
}

/// Something locked was used (`FixedItem` / `FixedDoor` `locked`): its `lockedSound` plays and
/// its `lockedTag` (else `lockedTextTag`) is shown.
#[derive(Message, Clone)]
pub struct LockedUse {
    pub record: String,
    pub at: Vec3,
}

pub struct ChestsPlugin;

impl Plugin for ChestsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ChestOpened>()
            .add_message::<LockedUse>()
            .add_systems(
                Update,
                open_chests
                    .run_if(resource_exists::<crate::lore::LoreCodex>)
                    .after(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                (story_locks, radius_locks, shrine_timers).chain(),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            test_open
                .before(open_chests)
                .after(gd_game_core::player::PlayerSet),
        );
    }
}

/// A chest of `record` (its `mesh` is needed) at `tf`; `opened`: opened earlier this session.
pub fn spawn_chest(
    commands: &mut Commands,
    db: &gd_formats::vfs::Database,
    tf: Transform,
    record: &str,
    key: ChestKey,
    proxy_level: i32,
    opened: bool,
) -> Option<Entity> {
    let rec = db.get(record)?;
    rec.str("mesh")?;
    debug!("chest {key:?} {record} at {:?}", tf.translation);
    let chest = Chest {
        record: record.to_owned(),
        key,
        opened,
        proxy_level,
        radius: gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS),
        locked: rec.bool("locked"),
        lock_radius: rec.f32("LockedRadius").filter(|r| *r > 0.0001).map(|r| {
            let class = match rec.str("LockedClassification").unwrap_or("") {
                "Common" => 0,
                "Champion" => 1,
                "Hero" => 2,
                "Boss" => 3,
                "Quest" => 4,
                _ => 999,
            };
            (r, class)
        }),
        shrine: rec.class == "FixedItemShrine",
        one_time: rec.bool("oneTimeUse"),
        phase: if rec.class == "FixedItemShrine" && opened {
            ShrinePhase::Spent
        } else {
            ShrinePhase::Idle
        },
    };
    let interact = crate::player_orders::Interactable::fixed(chest.radius);
    let mut e = commands.spawn((
        tf,
        GlobalTransform::from(tf),
        chest,
        interact,
        Name::new(format!("chest {}", record.rsplit('/').next().unwrap_or(""))),
    ));
    // Level and accessory chests go with their level (`usize::MAX`: a story proxy's stay).
    if let ChestKey::Level(l, _) | ChestKey::Accessory(l, _, _) = key
        && l != usize::MAX
    {
        e.insert(gd_game_core::levels::LevelMember(l));
    }
    Some(e.id())
}
