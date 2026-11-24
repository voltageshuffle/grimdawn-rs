//! Loot chests: opening one (in reach, unlocked) drops its loot at the
//! container level once a session; story locks; culling with the level.
//! The game shows them (model, idle / open animation); a click is the player's interact order
//! (`player_orders::InteractTarget`), which [`open_chests`] takes once in reach (`player_orders::usable_range`:
//! player radius + chest radius (`Actor::GetRadius`, `combat::get_radius`, kept in [`Chest::radius`]: the hover
//! pick reads the same) + `usableRange` (`combat::fixed_usable_range`: 1 m for the many chests without one),
//! +1 m at the walk's end, 3D).
//!
//! `FixedItemContainer::Open` (fields from
//! `FixedItemContainer::Load`): opening plays the record's
//! `openSound` pak at the chest's coords, starts `openAnimation`, and, when
//! `openFxPakName` is set, creates that object (cached at load) and adds it
//! to the world at the chest's coords (`World::AddEntity`, not attached);
//! then the loot is generated. Here: [`PlaySound`] and
//! [`gd_game_core::visuals::spawn_fx`] at the chest's transform. The loot is thrown
//! one item at a time from `FixedItem::GetLootDropCoords` with the record's
//! `actorRadius` (`crate::loot::fixed_drop_point`, `Dropper::one_by_one`).
//!
//! The radius lock (`FixedItemContainer::UpdateSelf`, `CheckRadiusClear`,
//! fields from `Load`): a container with `LockedRadius` (> 0.0001) that was not opened yet sets its `locked`
//! flag every 500 ms: locked while a live `Monster` (`World::GetEntitiesInSphere` of that radius around it)
//! has a classification (`ResolveMonsterClassificationEnum`) at or above its `LockedClassification`, a whole
//! string: `Common` 0, `Champion` 1, `Hero` 2, `Boss` 3, `Quest` 4, anything else 999 (never locks). The boss
//! chests (`locked` 1, radius 100, `Quest`, `tagChestLockedBoss`) unlock so once their boss is dead
//! ([`radius_locks`]; PORT: the monsters' centres, one timer for every chest). The same update shows the
//! record's `IdleEffect` until the chest opens and its `LockedEffect` while locked (grimdawn `chests.rs`).
//!
//! Food crates (`FixedItemShrine`: `interactive/constitution_foodcrate{a,b}01`, 19 placed in levels and 29 from
//! the `pool_constitution_foodcratea01` accessory pool) are used like chests ([`Chest::shrine`]).
//! Game.dll:
//! - `FixedItemShrineController::LoadFromDatabase`: `oneTimeUse`, `shrineActiveTime` /
//!   `shrineDormantTime` / `shrineSampleTime` (s → ms), `shrineRadius`. `TouchedByActor` (idle
//!   only): the crate goes idle → active (state 5) with the sample timer at 0 and the active timer at
//!   `shrineActiveTime`; `Update` samples while active: `GetTargetsInRadius` of `shrineRadius`
//!   around the crate for the user's side (its flags not decoded) → `FixedItemTargetConfigCmd` → `FixedItem::UseOnTarget`
//!   → `GiveBonusToTargets`: `Character::TakeBonus` with the record's bonus block
//!   (constitution: `crate::constitution`). The sample timer restarts at `shrineSampleTime` and both timers
//!   count down together, so with the stock 1 s / 1 s the active timer runs out first: one bonus per use.
//!   Then dormant for `shrineDormantTime`, back to idle only without `oneTimeUse` (1 in both records).
//! - `SetFixedItemState`: idle → `idleAnimation`, `StartIdleEffect` (`IdleLoopingEffect`
//!   attached at `IdleLoopingAttachment`); use → `GoIdleToActive` (`idleToActiveAnimation`,
//!   `idleToActiveSound` at the crate, `idleToActiveFxPakName`) and `StartActiveEffect`: the idle
//!   effects stop emitting and stay in the world (`PlaceEffectsInWorld`), `ActiveOneShotEffect` starts, with
//!   `ActiveMeshFX`. The stock crates have no active / dormant animations, so the crate rests opened.
//!
//! Here: the use sound and `ActiveOneShotEffect` at the crate, and the player's constitution bonus when the
//! player stands within `shrineRadius` (grimdawn `chests.rs` shows the animations and the idle effect).
//! PORT: the sample is taken at once (the engine's first controller update after the touch); only the
//! player is a target (pets have no constitution here, and the stock crates' other bonuses are 0); a crate
//! used is spent for the session like an opened chest (`oneTimeUse` in every stock record); no loot
//! (`TouchedByActor` → `LoadDropLoot`: the stock crates have no loot table).
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
    /// `Actor::GetRadius` (`combat::get_radius`): the use range's and the hover pick's.
    pub radius: f32,
    /// The record's `locked` (see `doors::LockedUse`); story `SetLocked` changes it.
    pub locked: bool,
    /// `LockedRadius` and the `LockedClassification` level (module doc, the radius lock).
    pub lock_radius: Option<(f32, u32)>,
    /// A `FixedItemShrine` (a food crate, module doc): its use gives a bonus, not loot.
    pub shrine: bool,
}

impl Chest {
    /// `FixedItemContainer::IsOfInterest`: its state
    /// is 1 (closed; `SetFixedItemState` → 3 opens it). A locked chest is
    /// still of interest (a click tells it is locked).
    pub fn is_of_interest(&self) -> bool {
        !self.opened
    }
}

/// A story chest was opened (the story runs its `onOpen` hooks).
#[derive(Message, Clone, Debug)]
pub struct ChestOpened(pub u32);

/// A path to beside a chest: chests usually stand on cells the nav data
/// blocks, so the goal is the chest, else the first reachable point on a
/// ring around it.
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
            Option<&mut crate::constitution::Constitution>,
        ),
        With<Player>,
    >,
    // Option: level 1 when absent.
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    world: Res<gd_game_core::levels::Levels>,
    nav: Res<Nav>,
    data: Res<gd_game_core::data::GameData>,
    (mut opened_out, mut stats): (
        MessageWriter<ChestOpened>,
        MessageWriter<crate::play_stats::PlayStat>,
    ),
    mut locked_out: MessageWriter<LockedUse>,
    mut sound: MessageWriter<PlaySound>,
    mut loot: crate::loot::ContainerLoot,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
) {
    let Ok((ptf, mut path, mut interact, pcb, ch, constitution)) = players.single_mut() else {
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
    // Use range (`player_orders::usable_range`): +1 m only once the walk has ended.
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
        // The player's locked-chest voice (`char_sounds::locked_vox`, `Player::PlayLockedChestVox`).
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
    chest.opened = true;
    states.opened.insert(chest.key);
    info!("chest {} opened at {at:.1}", chest.record);
    stats.write(crate::play_stats::PlayStat::ChestOpened(
        chest.record.clone(),
    ));
    if chest.shrine {
        // A food crate: its sound and effect, the bonus (module doc, "Food crates").
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
        let radius = rec.f32("shrineRadius").unwrap_or(0.0);
        if ptf.translation.distance(at) <= radius
            && let (Some(cb), Some(mut c)) = (pcb, constitution)
        {
            let f = |k: &str| rec.f32(k).unwrap_or(0.0);
            c.take_bonus(
                cb.profile.sheet.constitution,
                f("bonusConstitutionPercent"),
                f("bonusConstitutionPoints"),
            );
            info!(
                "food crate {}: constitution {:.0}",
                chest.record.rsplit('/').next().unwrap_or(""),
                c.current
            );
        }
        return;
    }
    // Open sound and FX (module doc).
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
    // Loot at the container level (see `container_level`).
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
    // The chest's lore note (PORT, `lore` module doc).
    let note = rec
        .str("perPartyMemberDropItemName")
        .filter(|n| !n.is_empty())
        .map(|n| gd_formats::save::item::Item {
            base_name: n.to_owned(),
            stack_count: 1,
            seed: rng.next().max(1),
            ..Default::default()
        });
    // `FixedItem::GetLootDropCoords` / `GetLootDropRadius` (`crate::loot` doc).
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

/// `GD_OPEN_CHESTS` (hooks.rs).
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

/// The radius lock (module doc).
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

/// Something locked was used (`FixedItem` / `FixedDoor` `locked`,
/// loaded from the record and honoured for every door: `FixedDoor::RequestToUse`
/// checks the flag, which only the record, `SetLocked` (scripts),
/// `QuestCommandLockDoor` and a restored state change): its `lockedSound`
/// plays and its `lockedTag` (else `lockedTextTag`) is shown.
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
                (test_open, open_chests)
                    .chain()
                    .after(gd_game_core::player::PlayerSet),
            )
            .add_systems(Update, (story_locks, radius_locks).chain());
    }
}

/// A chest of `record` (its `mesh` is needed) at `tf`; `opened`: opened
/// earlier this session.
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
    };
    let interact = crate::player_orders::Interactable::fixed(chest.radius);
    let mut e = commands.spawn((
        tf,
        GlobalTransform::from(tf),
        chest,
        interact,
        Name::new(format!("chest {}", record.rsplit('/').next().unwrap_or(""))),
    ));
    // Level and accessory chests go with their level (`usize::MAX`: a story proxy's stay; story chests are the
    // story's).
    if let ChestKey::Level(l, _) | ChestKey::Accessory(l, _, _) = key
        && l != usize::MAX
    {
        e.insert(gd_game_core::levels::LevelMember(l));
    }
    Some(e.id())
}
