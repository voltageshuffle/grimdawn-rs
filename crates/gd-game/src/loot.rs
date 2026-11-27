//! Rewards for kills: item drops and gold on the ground, picking them up
//! (rules in `gd_sim::items::loot`).
//!
//! Drops are thrown (`gd_sim::loot_toss`, `ItemDropper::Update`): a [`Dropper`] releases them 250 ms
//! after the kill or the opening (monsters and destructibles all at once, chests one every 200 ms);
//! each leaves the drop point with the engine's toss speed ([`Tossed`]: not pickable until released)
//! and flies as a rigid body ([`toss_flight`]) until it touches the terrain or static scenery or its
//! centre of mass reaches the walkable ground, which is when it gets its loot beam. The drop point is
//! `GetLootDropCoords`: [`monster_drop_point`] or [`fixed_drop_point`]. PORT: a named attach point is
//! taken in the mesh's bind pose (the engine's is the posed one); a drop with no container
//! ([`drop_loot`]) starts 1 m above the given ground point.
//!
//! Picking up (`CharPickUpConfigCmd::Execute`, [`pick_up`]): a Broken or already-carried
//! `cannotPickUpMultiple` item is refused; otherwise `OnPickup` fires and the item leaves the ground
//! BEFORE it is placed, then [`give_item`] (`Player::GiveItemToCharacter`).
use crate::combat::MonsterKilled;
use crate::movement::MovePath;
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::combat::Combatant;
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player};
use gd_game_core::progress::PlayerProgress;
use gd_rules::Rng;
use gd_sim::items::loot::{auto_pickup_radius, drop_sound, gold_with_bonus, rarity};
use gd_sim::items::{Classification, ItemDb};

/// Epic / Legendary items containers dropped this play session.
#[derive(Resource, Default)]
pub struct DroppedEpics(pub gd_sim::items::loot::DroppedEpics);

/// What container drops read beyond the container: session, Epics / Legendaries dropped, known
/// blueprints and challenge areas.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ContainerLoot<'w> {
    pub session: crate::session::Session<'w>,
    pub dropped: ResMut<'w, DroppedEpics>,
    /// Option: no stash before `init_stash`.
    pub stash: Option<Res<'w, gd_game_core::stash::Stash>>,
    /// Option: without loaded levels (tests) no container is in a challenge area.
    pub levels: Option<Res<'w, gd_game_core::levels::Levels>>,
}

impl ContainerLoot<'_> {
    /// `container`'s drops at `level` for a player of `player_level`, the container standing at `at`
    /// (for its challenge area).
    #[allow(clippy::too_many_arguments)]
    pub fn drops(
        &mut self,
        idb: &ItemDb,
        container: &str,
        level: u32,
        player_level: u32,
        at: Option<Vec3>,
        rng: &mut Rng,
    ) -> Vec<gd_sim::items::LootRoll> {
        let params = self.session.loot(level, player_level);
        let known = self
            .stash
            .as_ref()
            .map(|s| s.formulas.formula_records())
            .unwrap_or_default();
        let area = at.and_then(|p| {
            let levels = self.levels.as_ref()?;
            crate::challenge_areas::challenge_area_at(levels, None, p).map(|a| a.1.to_owned())
        });
        gd_sim::items::loot::container_drops_in(
            idb,
            container,
            &params,
            area.as_deref(),
            &known,
            &mut self.dropped.0,
            rng,
        )
    }
}

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        // Kill drops use the level before the kill's experience.
        use crate::progress::{init_progress, kill_experience, story_progress};
        app.add_message::<ItemPickedUp>()
            .init_resource::<LevelItems>()
            // After the pickups: an item taken this frame is not saved too.
            .add_systems(
                Update,
                (stash_ground_items.after(pick_up), restore_ground_items),
            );
        app.add_systems(
            Update,
            on_kill
                .run_if(resource_exists::<PlayerProgress>)
                .run_if(resource_exists::<crate::lore::LoreCodex>)
                .after(gd_game_core::player::PlayerSet)
                .after(init_progress)
                .before(kill_experience),
        )
        .add_systems(
            Update,
            (
                story_rewards
                    .run_if(resource_exists::<PlayerProgress>)
                    .run_if(resource_exists::<gd_game_core::inventory::Inventory>),
                refill_energy.after(crate::player::rebuild_profile),
                pick_up.run_if(resource_exists::<PlayerProgress>),
            )
                .chain()
                .after(story_progress)
                .after(gd_game_core::inventory::InventorySet),
        )
        .add_systems(Update, (toss_flight, pickup_delay).before(pick_up));
    }
}

/// A ground item was picked up (`Item::OnPickup`, before it is placed).
#[derive(Message, Clone, Debug)]
pub struct ItemPickedUp {
    pub item: Item,
    pub at: Vec3,
}

#[derive(Component, Clone)]
pub struct GroundItem {
    pub item: Option<Item>,
    pub gold: u32,
    pub yaw: f32,
}

/// The ground items of the levels streamed out this session, by level (`Level::SaveState` /
/// `Item::ShouldSaveState`); the level's next load puts them back.
#[derive(Resource, Default)]
pub struct LevelItems(pub gd_formats::collections::HashMap<usize, Vec<SavedItem>>);

pub struct SavedItem {
    ground: GroundItem,
    at: Transform,
    tossed: Option<Tossed>,
    no_beam: bool,
    by_player: bool,
}

/// The ground items of an unloaded level leave the world into [`LevelItems`]. A level's placed note
/// only leaves: the level's load places it again unless taken.
#[allow(clippy::type_complexity)]
pub fn stash_ground_items(
    mut commands: Commands,
    levels: Res<gd_game_core::levels::Levels>,
    items: Query<(
        Entity,
        &GroundItem,
        Ref<crate::monsters::LevelProp>,
        &Transform,
        Option<&Tossed>,
        Has<NoBeam>,
        Has<crate::item_drop::DroppedByPlayer>,
        Has<crate::lore::PlacedNote>,
    )>,
    mut saved: ResMut<LevelItems>,
) {
    let all = levels.is_changed();
    for (e, ground, level, at, tossed, no_beam, by_player, placed) in &items {
        if !(all || level.is_added()) || levels.is_loaded(level.0) {
            continue;
        }
        commands.entity(e).try_insert(gd_game_core::levels::Unload);
        if placed {
            continue;
        }
        saved.0.entry(level.0).or_default().push(SavedItem {
            ground: ground.clone(),
            at: *at,
            tossed: tossed.cloned(),
            no_beam,
            by_player,
        });
    }
}

/// A ground item its level's load put back (`Entity::IsRestored`): no `onDrop`.
#[derive(Component)]
pub struct Restored;

/// A level that loaded again gets its saved ground items back (`Level::RestoreState`). A lore note
/// already in the codex is dropped (`ItemNote::InitialUpdate` hides it).
pub fn restore_ground_items(
    mut commands: Commands,
    levels: Res<gd_game_core::levels::Levels>,
    mut saved: ResMut<LevelItems>,
    // Option: the codex is made at the session start; without it no note is known yet.
    codex: Option<Res<crate::lore::LoreCodex>>,
) {
    if !levels.is_changed() || saved.0.is_empty() {
        return;
    }
    saved.0.retain(|&level, items| {
        if !levels.is_loaded(level) {
            return true;
        }
        for s in items.drain(..) {
            let known_note = |it: &Item| codex.as_ref().is_some_and(|c| c.knows(&it.base_name));
            if s.ground.item.as_ref().is_some_and(known_note) {
                continue;
            }
            let mut e = commands.spawn((
                s.at,
                GlobalTransform::from(s.at),
                s.ground,
                Name::new("ground item"),
                crate::monsters::LevelProp(level),
                Restored,
            ));
            if let Some(t) = s.tossed {
                e.insert(t);
            }
            if s.no_beam {
                e.insert(NoBeam);
            }
            if s.by_player {
                e.insert(crate::item_drop::DroppedByPlayer);
            }
        }
        false
    });
}

#[allow(clippy::too_many_arguments)]
pub fn on_kill(
    mut commands: Commands,
    mut events: MessageReader<MonsterKilled>,
    progress: Res<PlayerProgress>,
    players: Query<&Loadout, With<Player>>,
    data: Res<GameData>,
    mut rng: Local<Option<Rng>>,
    mut fresh: gd_game_core::session::NewSession,
    worn: Query<&crate::monsters::Worn>,
    session: crate::session::Session,
    codex: Res<crate::lore::LoreCodex>,
    (rt, monsters, challenged, bodies): (
        // Option: without a story runtime no monster is in a Shattered Realm dungeon.
        Option<NonSend<gd_game_core::story::StoryRuntime>>,
        Query<&gd_game_core::monsters::Monster>,
        Query<&crate::challenge_areas::InChallengeArea>,
        Query<(&Transform, &gd_game_core::character::Character)>,
    ),
) {
    // A new session rolls a fresh loot stream.
    if fresh.begun() {
        *rng = None;
    }
    let idb = &data.item_db();
    let rng = rng.get_or_insert_with(|| Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF)));
    for ev in events.read() {
        let player_level = progress.progress.level;
        // `PlayerManagerServer::HandleBonus`: + round(amount × mod).
        let asc = session.ascendant();
        let params = session.loot(ev.level.max(1) as u32, player_level as u32);
        // The gear it wore (Broken pieces are destroyed, not dropped), then the misc slots.
        let mut items: Vec<Item> = match worn.get(ev.entity) {
            Ok(w) => w
                .items
                .iter()
                .zip(&w.resolved)
                .filter(|(_, r)| rarity(r) != Classification::Broken)
                .map(|(i, _)| i.clone())
                .collect(),
            Err(_) => Vec::new(),
        };
        // `Monster.challengeArea`: its weights and extra drop.
        let area = challenged.get(ev.entity).ok().map(|a| a.record.as_str());
        items.extend(
            gd_sim::items::loot::monster_misc_in(idb, &ev.record, &params, area, rng)
                .iter()
                .map(|d| d.to_item()),
        );
        let rec = data.db.get(&ev.record);
        // Lore notes (`rand() % 101`); one already read is rolled and seeded but not dropped.
        if let Some(r) = &rec {
            items.extend(
                crate::lore::monster_note_drops(r, || gd_game_core::rng::u32(0..101))
                    .into_iter()
                    .map(|n| Item {
                        base_name: n,
                        stack_count: 1,
                        seed: rng.next().max(1),
                        ..Default::default()
                    })
                    .filter(|it| !codex.knows(&it.base_name)),
            );
        }
        let mut drops = items;
        // A Shattered Realm dungeon monster (`ControllerMonster::DropLoot`): with `disableMonsterDrops`
        // only `allowedItems` drop and no gold; essences are used at once; no ascended drops or reagents.
        let dungeon = rt.as_ref().and_then(|rt| {
            let story = rt.story.borrow();
            let d = story.endless.dungeon.as_ref()?;
            let slot = monsters.get(ev.entity).ok()?.slot;
            if !story
                .endless
                .activated
                .iter()
                .any(|(_, k)| *k == (slot.0, slot.1))
            {
                return None;
            }
            gd_story::endless::Ruleset::load(
                &data.db,
                &d.ruleset,
                gd_story::endless::Ruleset::slot(&session.mode()),
            )
        });
        if let (Some(rs), Some(rt)) = (&dungeon, &rt) {
            if rs.disable_monster_drops {
                drops.retain(|i| rs.allowed_items.contains(&i.base_name.to_ascii_lowercase()));
            }
            let (essences, rest): (Vec<Item>, Vec<Item>) = drops.into_iter().partition(|i| {
                data.db
                    .get(&i.base_name)
                    .is_some_and(|r| r.class == "OneShot_EndlessDungeon")
            });
            drops = rest;
            rt.story
                .borrow_mut()
                .endless
                .essence_queue
                .extend(essences.into_iter().map(|i| i.base_name));
        }
        let gold = rec
            .as_ref()
            .and_then(|r| {
                r.str("goldGenerator")
                    .map(|g| (g, r.f32("goldGeneratorChance").unwrap_or(100.0)))
            })
            .map(|(g, chance)| gd_sim::progress::gold_drop(&data.db, g, chance, ev.level, rng))
            .unwrap_or_default()
            .into_iter()
            .map(|g| gd_sim::ascendant::apply_bonus(g as u64, asc.gold) as u32)
            .filter(|_| !dungeon.as_ref().is_some_and(|rs| rs.disable_monster_drops))
            .collect::<Vec<_>>();
        // Mastery tables by the player's masteries (class number − 1, `Skill_Mastery::Load`).
        let trees = data.skill_trees();
        let masteries: Vec<usize> = players
            .single()
            .map(|l| {
                trees
                    .learned(&l.skills)
                    .map(|t| t.class.saturating_sub(1) as usize)
                    .collect()
            })
            .unwrap_or_default();
        for it in drops.iter_mut().filter(|_| dungeon.is_none()) {
            let Some(r) = gd_sim::items::resolve(idb, it) else {
                continue;
            };
            if let Some(a) = gd_sim::ascendant::ascend_drop(
                idb,
                asc,
                session.mode(),
                &r.class,
                r.classification.name(),
                &masteries,
                rng,
            ) {
                debug!(
                    "ascended drop {} + {}",
                    it.base_name.rsplit('/').next().unwrap_or(""),
                    a.rsplit('/').next().unwrap_or("")
                );
                it.ascendant_name = a;
            }
        }
        let class = rec
            .as_ref()
            .and_then(|r| r.str("monsterClassification"))
            .unwrap_or("");
        for (item, chance) in gd_sim::ascendant::reagent_drops(&data.db, asc, session.mode(), class)
            .into_iter()
            .filter(|_| dungeon.is_none())
        {
            if gd_sim::ascendant::roll_float(rng, chance) {
                debug!(
                    "ascension reagent {} ({chance:.1} %)",
                    item.rsplit('/').next().unwrap_or("")
                );
                drops.push(Item {
                    base_name: item,
                    stack_count: 1,
                    seed: rng.next().max(1),
                    ..Default::default()
                });
            }
        }
        if !drops.is_empty() || !gold.is_empty() {
            info!(
                "{}: {} items, gold {:?}",
                ev.record.rsplit('/').next().unwrap_or(""),
                drops.len(),
                gold
            );
        }
        // The dying body's drop point, else its feet.
        let drop = bodies.get(ev.entity).map_or(ev.position, |(tf, ch)| {
            let attach = rec
                .as_ref()
                .and_then(|r| r.str("controller"))
                .and_then(|c| data.db.get(c))
                .and_then(|c| c.str("lootDropAttachPointName"));
            monster_drop_point(tf, ch, attach)
        });
        drop_loot_by(&mut commands, idb, drop, drops, &gold, Dropper::MONSTER);
    }
}

/// `ControllerMonster::GetLootDropCoords`: the monster's `attach` point (`lootDropAttachPointName`;
/// PORT: bind pose) when its mesh has it, else its position moved by its body's centre of mass.
pub fn monster_drop_point(
    tf: &Transform,
    ch: &gd_game_core::character::Character,
    attach: Option<&str>,
) -> Vec3 {
    if let Some(name) = attach.filter(|n| !n.is_empty()) {
        // `Name` ids are case-sensitive.
        return ch
            .body
            .attach_points()
            .iter()
            .any(|p| p.name == name)
            .then(|| gd_game_core::casting::attach_point_bind(&ch.body, name))
            .flatten()
            .map_or(tf.translation, |l| tf.transform_point(l));
    }
    tf.translation
        + tf.rotation * crate::monster_skills::center_of_mass(ch.is_player, 0.0, ch.scale, &ch.body)
}

/// `FixedItem` / `Destructible` / `StaticShrine::GetLootDropCoords`: the `LootDrop` attach point, else
/// the mesh's bounding-box centre, at least 1 m over the floor; with no mesh, `pos` 1 m up.
pub fn fixed_drop_point(
    nav: &Nav,
    pos: Vec3,
    rot: Quat,
    body: Option<&gd_formats::msh::Mesh>,
) -> Vec3 {
    use gd_sim::loot_toss as toss;
    let Some(body) = body else {
        return pos + Vec3::Y * toss::MIN_DROP_HEIGHT;
    };
    let local = body
        .attach_points()
        .iter()
        .any(|p| p.name == "LootDrop")
        .then(|| gd_game_core::casting::attach_point_bind(body, "LootDrop"))
        .flatten()
        .unwrap_or_else(|| crate::monster_skills::center_of_mass(false, 0.0, 1.0, body));
    let p = pos + rot * local;
    let floor = nav.0.ground(p.to_array()).unwrap_or(pos.y);
    p.with_y(toss::raise_drop(p.y, floor))
}

/// How a container releases its drops.
#[derive(Clone, Copy, Debug)]
pub struct Dropper {
    /// `ItemDropper.batchSize`.
    pub batch: usize,
    /// `GetLootDropRadius`: the start's offset along the toss and up.
    pub radius: f32,
}

impl Dropper {
    /// `ControllerMonster::DropLoot`: everything at once, radius 0.
    pub const MONSTER: Dropper = Dropper {
        batch: 1000,
        radius: 0.0,
    };

    /// A chest's `lootGen` event or a shrine's cleansing (`StaticShrine::CleanseShrine`): one item
    /// per release.
    pub fn one_by_one(actor_radius: f32) -> Dropper {
        Dropper {
            batch: 1,
            radius: gd_sim::loot_toss::drop_radius(actor_radius),
        }
    }

    /// `Destructible::BreakApart`: everything at once.
    pub fn all_at_once(actor_radius: f32) -> Dropper {
        Dropper {
            batch: 1000,
            radius: gd_sim::loot_toss::drop_radius(actor_radius),
        }
    }
}

/// A thrown ground item that has not landed yet: it waits `wait` seconds, then flies at velocity `v`
/// as `body` (`com`: centre of mass in its own frame) until it lands ([`toss_flight`]).
#[derive(Component, Clone, Debug)]
pub struct Tossed {
    pub wait: f32,
    pub v: Vec3,
    pub body: Option<gd_sim::drop_body::DropBody>,
    pub com: Vec3,
    pub clock: gd_sim::drop_body::Clock,
    pub updates: u32,
}

impl Tossed {
    pub fn new(v: [f32; 3], wait: f32) -> Tossed {
        Tossed {
            wait,
            v: Vec3::from(v),
            body: None,
            com: Vec3::ZERO,
            clock: Default::default(),
            updates: 0,
        }
    }
}

/// The record whose mesh a gold pile has. PORT: the one stock `OneShot_Gold` record (how the engine picks it
/// is not decompiled).
pub const GOLD_RECORD: &str = "records/items/misc/moneyobject01.dbr";

/// `Entity::PhysicsUpdate`: a body leaves the simulation after 2000 physics updates.
pub const MAX_PHYSICS_UPDATES: u32 = 2000;

/// `Actor::PhysicsUpdate`: a body falling faster than this (m/s) sleeps and `Item::PhysicsPost` puts
/// the item on the ground under it.
pub const FALL_LIMIT: f32 = 25.0;

/// A ground item that left the simulation without landing ([`toss_flight`]): it never gets a loot beam.
#[derive(Component, Clone, Copy, Debug)]
pub struct NoBeam;

/// An item's collision box (`Actor::GetCollisionBox`): centre of mass in its own frame (mesh
/// bounding-box centre, unscaled), half extents (× record `scale`, at least 0.1 m) and `physicsFriction`.
pub fn item_box(
    data: &GameData,
    bodies: &mut gd_game_core::creature::BodyCache,
    ground: &GroundItem,
) -> (Vec3, [f32; 3], f32) {
    let record = ground
        .item
        .as_ref()
        .map_or(GOLD_RECORD, |i| i.base_name.as_str());
    let rec = data.db.get(record);
    let scale = rec
        .as_ref()
        .and_then(|r| r.f32("scale"))
        .filter(|s| *s > 0.0)
        .unwrap_or(1.0);
    let bbox = rec
        .as_ref()
        .and_then(|r| r.str("mesh"))
        .and_then(|m| bodies.get(data, m))
        .and_then(|m| m.bounding_box);
    let (com, half) = bbox.map_or((Vec3::ZERO, Vec3::ZERO), |(lo, hi)| {
        let (lo, hi) = (Vec3::from(lo), Vec3::from(hi));
        ((lo + hi) * 0.5, (hi - lo) * 0.5)
    });
    let friction = rec
        .as_ref()
        .and_then(|r| r.f32("physicsFriction"))
        .unwrap_or(0.0);
    (
        com,
        gd_sim::drop_body::half_extents(half.to_array(), scale),
        friction,
    )
}

/// `NavManager::FindClosestPointOnPathMesh(p, 3)` (Detour's nearest polygon within (3, 10, 3) m and
/// the closest point on it).
pub fn path_mesh_point(nav: &Nav, p: Vec3) -> Option<Vec3> {
    const REACH: f32 = 3.0;
    const HEIGHT: f32 = 10.0;
    let nav = &nav.0;
    let c = nav.nearest(p.to_array(), REACH)?;
    let (pos, half) = (Vec3::from(nav.pos(c)), nav.cell_size(c) * 0.5);
    let q = Vec3::new(
        p.x.clamp(pos.x - half, pos.x + half),
        pos.y,
        p.z.clamp(pos.z - half, pos.z + half),
    );
    ((q.y - p.y).abs() <= HEIGHT && (q - p).with_y(0.0).length() <= REACH).then_some(q)
}

/// Puts items and gold piles on the ground around `at` as a monster drops them, from 1 m above it;
/// items that do not resolve are dropped.
pub fn drop_loot(
    commands: &mut Commands,
    idb: &ItemDb,
    at: Vec3,
    items: impl IntoIterator<Item = Item>,
    gold: &[u32],
) {
    let drop = at + Vec3::Y * gd_sim::loot_toss::MIN_DROP_HEIGHT;
    drop_loot_by(commands, idb, drop, items, gold, Dropper::MONSTER);
}

/// Items and gold piles released by `dropper` from its drop point `drop`.
pub fn drop_loot_by(
    commands: &mut Commands,
    idb: &ItemDb,
    drop: Vec3,
    items: impl IntoIterator<Item = Item>,
    gold: &[u32],
    dropper: Dropper,
) {
    let mut i = 0;
    for item in items {
        if gd_sim::items::resolve(idb, &item).is_none() {
            continue;
        }
        spawn_ground(commands, drop, Some(item), 0, dropper, i);
        i += 1;
    }
    for &g in gold {
        spawn_ground(commands, drop, None, g, dropper, i);
        i += 1;
    }
}

/// The `i`-th drop of `dropper` from the drop point `drop`.
pub fn spawn_ground(
    commands: &mut Commands,
    drop: Vec3,
    item: Option<Item>,
    gold: u32,
    dropper: Dropper,
    i: usize,
) -> Entity {
    use gd_sim::loot_toss as toss;
    // The dropper's angle stays 0, so every toss goes along +X.
    let start = Vec3::from(toss::toss_start(drop.to_array(), dropper.radius, 0.0));
    let wait = toss::release_ms(i, dropper.batch, 1) / 1000.0;
    spawn_thrown(
        commands,
        start,
        toss::toss_velocity(0.0),
        wait,
        GroundItem {
            item,
            gold,
            yaw: 0.0,
        },
    )
}

/// A ground item at `start`, released after `wait` seconds at velocity `v`, turned by a random yaw.
pub fn spawn_thrown(
    commands: &mut Commands,
    start: Vec3,
    v: [f32; 3],
    wait: f32,
    ground: GroundItem,
) -> Entity {
    let yaw = gd_sim::loot_toss::random_yaw(gd_game_core::rng::u32(0..=0x7FFF));
    // With its GlobalTransform: pickup reads it before transform propagation.
    let tf = Transform::from_translation(start).with_rotation(Quat::from_rotation_y(yaw));
    commands
        .spawn((
            tf,
            GlobalTransform::from(tf),
            GroundItem { yaw, ..ground },
            Tossed::new(v, wait),
            Name::new("ground item"),
        ))
        .id()
}

/// Thrown items wait for their release, fly as a rigid body and land ([`Tossed`], `gd_sim::drop_body`).
///
/// Each frame runs the body's fixed steps against terrain and static scenery (a contact in any lands
/// it), then, as `Item::PhysicsUpdate`, moves the item by the offset `d` from its centre of mass to
/// the closest path-mesh point: horizontally only while that point is below the centre, landed when it
/// is at or above it. Landed: drop sound — the water sound (`dropSoundWater`) when the resting point
/// sits on a water surface (`Item::PlayDropSoundWorld`). Still flying after [`MAX_PHYSICS_UPDATES`] or falling faster
/// than [`FALL_LIMIT`]: leaves the simulation without sound or beam ([`NoBeam`]; `Item::CreateBeamLoop`
/// only runs on landing).
#[allow(clippy::too_many_arguments)]
pub fn toss_flight(
    mut commands: Commands,
    time: Res<Time>,
    mut items: Query<(
        Entity,
        &mut Tossed,
        &mut Transform,
        &mut GlobalTransform,
        &GroundItem,
    )>,
    data: Res<GameData>,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
    (coll, nav): (Res<crate::collision::WorldCollision>, Res<Nav>),
    mut sounds: MessageWriter<gd_game_core::visuals::PlaySoundOn>,
    mut tris: Local<Vec<gd_sim::drop_body::Tri>>,
) {
    for (e, mut toss, mut tf, mut gt, ground) in &mut items {
        let mut dt = time.delta_secs();
        if toss.wait > 0.0 {
            toss.wait -= dt;
            if toss.wait > 0.0 {
                continue;
            }
            dt = -toss.wait;
            toss.wait = 0.0;
        }
        let toss = &mut *toss;
        if toss.body.is_none() {
            let (com, half, friction) = item_box(&data, &mut bodies, ground);
            toss.com = com;
            toss.body = Some(gd_sim::drop_body::DropBody::new(
                (tf.translation + tf.rotation * com).to_array(),
                tf.rotation.to_array(),
                toss.v.to_array(),
                half,
                friction,
            ));
        }
        let Some(body) = toss.body.as_mut() else {
            continue;
        };
        let n = toss.clock.steps(dt);
        let (lo, hi) = body.aabb();
        let reach = Vec3::splat(gd_sim::drop_body::CONTACT_THRESHOLD + 0.1)
            + Vec3::from(body.vel).abs() * (n as f32 * gd_sim::drop_body::STEP)
            + Vec3::Y * (gd_sim::drop_body::GRAVITY * (n as f32 * gd_sim::drop_body::STEP).powi(2));
        tris.clear();
        coll.triangles_near(Vec3::from(lo) - reach, Vec3::from(hi) + reach, &mut tris);
        // The levels come in map order: by object, for a deterministic solve.
        tris.sort_by_key(|t| t.owner);
        // A contact in any step lands it; the frame's later steps still run.
        let mut landed = false;
        for _ in 0..n {
            landed |= body.step(&tris);
        }
        toss.updates += 1;
        let rot = Quat::from_array(body.rot);
        let com = Vec3::from(body.pos);
        let mut origin = com - rot * toss.com;
        if let Some(q) = path_mesh_point(&nav, com) {
            let mut d = q - com;
            if d.y >= 0.0 {
                landed = true;
            } else {
                d.y = 0.0;
            }
            origin += d;
        }
        // `Entity::PhysicsUpdate`: out of the simulation without landing.
        let fell = body.vel[1] < -FALL_LIMIT;
        if fell
            && let Some(y) = coll
                .terrain_ground(origin)
                .or_else(|| nav.0.ground(origin.to_array()))
        {
            origin.y = y;
        }
        let stopped = !landed && (fell || toss.updates > MAX_PHYSICS_UPDATES);
        tf.translation = origin;
        tf.rotation = rot;
        *gt = GlobalTransform::from(*tf);
        if stopped {
            commands.entity(e).try_remove::<Tossed>().try_insert(NoBeam);
            continue;
        }
        if !landed {
            continue;
        }
        commands.entity(e).try_remove::<Tossed>();
        // `Item::CollisionCallback` / `Item::PhysicsUpdate` on landing: the sound follows the
        // contact's `PhysicsSurface` (`Item::PlayDropSoundWorld`), water when the resting point
        // sits on a water surface (`Ground::water_below`, as `WorldCollision::surface_below`).
        if let Some(item) = ground.item.as_ref() {
            let idb = &data.item_db();
            let in_water = coll.surface_below(origin) == gd_sim::surface::Surface::Water as u32;
            if let Some(pak) = drop_sound(idb, item, in_water) {
                sounds.write(gd_game_core::visuals::PlaySoundOn {
                    pak,
                    on: e,
                    at: origin,
                });
            }
        }
    }
}

/// The auto-pickup delay of a new ground item (s): every auto-pickable class sets a 750 ms countdown
/// (`autoPickupDelay`) and its `CanAutoPickup` refuses until it reaches 0. A click pickup ignores it.
pub const PICKUP_DELAY: f32 = 0.75;

#[derive(Component)]
pub struct PickupDelay(pub f32);

/// Gives every new ground item its [`PickupDelay`] and counts it down once released.
pub fn pickup_delay(
    mut commands: Commands,
    time: Res<Time>,
    new: Query<Entity, Added<GroundItem>>,
    mut delays: Query<(Entity, &mut PickupDelay, Option<&Tossed>)>,
) {
    for e in &new {
        commands.entity(e).try_insert(PickupDelay(PICKUP_DELAY));
    }
    for (e, mut d, toss) in &mut delays {
        if toss.is_some_and(|t| t.wait > 0.0) {
            continue;
        }
        d.0 -= time.delta_secs();
        if d.0 <= 0.0 {
            commands.entity(e).try_remove::<PickupDelay>();
        }
    }
}

pub const MAX_GOLD: u64 = 2_000_000_000;

/// `Character::AddMoney` / `TakeBonus`: a gain caps at [`MAX_GOLD`], a loss never goes below 0. The one
/// place gold gains are clamped.
pub fn add_gold(gold: &mut u64, amount: i64) {
    *gold = (*gold as i64 + amount).clamp(0, MAX_GOLD as i64) as u64;
}

/// `Player::kAutoPickupPeriod` (50 ms).
pub const AUTO_PICKUP_PERIOD: f32 = 0.05;

/// Level-ups refill energy (`CharacterBio::Update`). Runs after `player::rebuild_profile` so the
/// refill is the new level's maximum.
pub fn refill_energy(
    mut leveled: MessageReader<gd_game_core::progress::LeveledUp>,
    mut players: Query<(&Combatant, &mut gd_game_core::casting::Caster)>,
) {
    for ev in leveled.read() {
        if let Ok((cb, mut c)) = players.get_mut(ev.player) {
            c.energy = cb.profile.sheet.usable_energy();
        }
    }
}

/// Quest/conversation/script rewards (`ScriptableAction_Give*`, Lua `Player:Give*`). `GiveItem`
/// creates `count` separate items with fresh seeds; a negative count takes that many
/// (`Player::TakeItemFromStashes`). `GiveRandomItem` rolls the loot table at the player's level and
/// difficulty. PORT: the engine rolls in `OnActivate` and the reward window reads the rolls back; the
/// port rolls here and hands them over via [`crate::quest_reward::RandomRewardRolls`]. Lua
/// `GiveItem`'s fourth bool gives a relic / component complete (`LuaPlayer::GiveItem`); the quest
/// action's `rollBonus` is never set, so a quest-given component stays incomplete.
#[allow(clippy::too_many_arguments)]
pub fn story_rewards(
    mut commands: Commands,
    mut events: MessageReader<gd_game_core::story::StoryEffect>,
    progress: Res<PlayerProgress>,
    mut inv: ResMut<gd_game_core::inventory::Inventory>,
    // Option: no stash before `init_stash`.
    mut stash: Option<ResMut<gd_game_core::stash::Stash>>,
    players: Query<
        (
            &Transform,
            &gd_game_core::character::Character,
            &gd_game_core::inventory::Requirements,
        ),
        With<Player>,
    >,
    data: Res<GameData>,
    session: crate::session::Session,
    mut rolls: ResMut<crate::quest_reward::RandomRewardRolls>,
) {
    use gd_story::story::Effect;
    let idb = &data.item_db();
    let Ok((t, ch, req)) = players.single() else {
        return;
    };
    let to = Receiver {
        center: t.translation + crate::monster_skills::character_center(&data, ch),
        req,
    };
    for ev in events.read() {
        let mut items = Vec::new();
        match &ev.0 {
            // Experience, levels, money and points: `gd_game::progress::story_progress`.
            Effect::GiveItem { record, count, .. } if *count < 0 => {
                let n = count.unsigned_abs();
                let removed = inv.remove_by_record(record, n);
                if removed < n
                    && let Some(stash) = stash.as_deref_mut()
                {
                    gd_game_core::stash::take(
                        std::slice::from_ref(record),
                        n - removed,
                        &mut inv,
                        stash,
                    );
                }
                info!("story: took {n} x {record} ({removed} from the bags)");
            }
            Effect::GiveItem {
                record,
                count,
                complete_relic,
            } => {
                let rec = data.db.get(record);
                let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
                for _ in 0..*count {
                    let mut item = Item {
                        base_name: record.clone(),
                        seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
                        stack_count: 1,
                        ..Default::default()
                    };
                    if *complete_relic
                        && let Some(r) = rec
                            .as_ref()
                            .filter(|r| r.class == "ItemRelic" || r.class == "ItemArtifact")
                    {
                        item.var1 = gd_sim::items::components::shards_needed(r);
                        item.relic_bonus = gd_sim::items::components::roll_bonus(idb, r, &mut rng);
                    }
                    items.push(item);
                }
            }
            Effect::GiveRandomItem { table, count } => {
                let level = progress.progress.level.max(1) as u32;
                let params = session.loot(level, level);
                let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
                let mut rolled = Vec::new();
                for _ in 0..*count {
                    match gd_sim::items::loot::story_random_item(idb, table, &params, &mut rng) {
                        Some(r) => rolled.push(r.to_item()),
                        None => warn!("story: loot table {table} gave nothing"),
                    }
                }
                rolls.push(table, rolled.clone());
                items.extend(rolled);
            }
            // `Player:TakeItem(record, count, bool)` (`QuestCommandTakeItem` → `Player::TakeItemFromStashes`):
            // the bool is the relic filter.
            Effect::TakeItem {
                record,
                count,
                extra,
            } => {
                let n = take_item_filtered(
                    record,
                    (*count).max(0) as u32,
                    *extra,
                    &mut inv,
                    stash.as_deref_mut(),
                    &data.db,
                );
                info!("story: took {n} x {record} (relic filter {extra})");
            }
            // `Player:GiveAffixItem(record, prefix, suffix)` (`LuaPlayer::GiveAffixItem`): the named affixes
            // on a fresh seed, resolved like any loot item.
            Effect::GiveAffixItem {
                record,
                prefix,
                suffix,
            } => {
                items.push(Item {
                    base_name: record.clone(),
                    prefix_name: prefix.clone(),
                    suffix_name: suffix.clone(),
                    seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
                    stack_count: 1,
                    ..Default::default()
                });
            }
            _ => {}
        }
        for item in items {
            give_item(&mut commands, &mut inv, &data, to, item, false);
        }
    }
}

/// `Player::TakeItemFromStashes` (Game.dll): the bags (not equipped gear), then the materials storage,
/// the private and the transfer stash. The bool picks the relic mode (`Character::TakeItemFromCharacter`):
/// true takes only complete relics / components by stack size; false counts a relic's shards (`var1`),
/// incomplete ones included. A whole stack taken removes the item; non-relics take the plain stack count.
fn take_item_filtered(
    record: &str,
    count: u32,
    extra: bool,
    inv: &mut gd_game_core::inventory::Inventory,
    stash: Option<&mut gd_game_core::stash::Stash>,
    db: &gd_formats::vfs::Database,
) -> u32 {
    let mut left = count;
    for b in &mut inv.bags {
        take_sack_filtered(&mut b.items, record, &mut left, extra, db);
    }
    if let Some(stash) = stash {
        for e in &mut stash.materials.entries {
            if left > 0 && e.record.eq_ignore_ascii_case(record) {
                let k = e.count.min(left);
                e.count -= k;
                left -= k;
            }
        }
        stash.materials.entries.retain(|e| e.count > 0);
        for p in stash.pages.iter_mut().chain(stash.transfer.iter_mut()) {
            take_sack_filtered(&mut p.items, record, &mut left, extra, db);
        }
    }
    if left < count {
        inv.changed();
    }
    count - left
}

fn take_sack_filtered(
    items: &mut Vec<gd_formats::inventory::Placed<Item>>,
    record: &str,
    left: &mut u32,
    extra: bool,
    db: &gd_formats::vfs::Database,
) {
    items.retain_mut(|p| {
        if *left == 0 || !p.item.base_name.eq_ignore_ascii_case(record) {
            return true;
        }
        let rec = db.get(&p.item.base_name);
        let relic = rec
            .as_ref()
            .is_some_and(|r| r.class == "ItemRelic" || r.class == "ItemArtifact");
        if relic {
            let complete = rec
                .as_ref()
                .is_some_and(|r| gd_sim::items::components::is_complete(r, p.item.var1));
            if extra && !complete {
                return true;
            }
            let avail = if extra {
                p.item.stack_count.max(1)
            } else {
                p.item.var1.max(1)
            };
            let n = avail.min(*left);
            *left -= n;
            let rem = avail - n;
            if rem == 0 {
                return false;
            }
            if extra {
                p.item.stack_count = rem;
            } else {
                p.item.var1 = rem;
            }
            true
        } else {
            let n = p.item.stack_count.max(1).min(*left);
            *left -= n;
            p.item.stack_count = p.item.stack_count.max(1) - n;
            p.item.stack_count > 0
        }
    });
}

/// Who [`give_item`] gives to: equip requirements and the centre a refused item is thrown from.
#[derive(Clone, Copy)]
pub struct Receiver<'a> {
    pub center: Vec3,
    pub req: &'a gd_game_core::inventory::Requirements,
}

/// `Player::GiveItemToCharacter`: [`Inventory::give`]; when nothing takes the item, `InventoryFull`
/// vox, tutorial page 33, and the item is destroyed when soulbound or untradeable, else re-thrown from
/// `center` as dropped by the player (never auto-picked again). True when the inventory took it.
pub fn give_item(
    commands: &mut Commands,
    inv: &mut gd_game_core::inventory::Inventory,
    data: &GameData,
    to: Receiver,
    item: Item,
    combine: bool,
) -> bool {
    let center = to.center;
    let Err(item) = inv.give(item, combine, to.req) else {
        return true;
    };
    commands.write_message(crate::error_vox::ErrorVox::new(
        crate::error_vox::VoxKind::InventoryFull,
        "",
    ));
    commands.write_message(crate::tutorials::UnlockTutorial(33));
    let rec = data.db.get(&item.base_name);
    if rec
        .as_ref()
        .and_then(|r| crate::item_drop::destroy_tag(r))
        .is_some()
    {
        info!("inventory full: {} is destroyed", item.base_name);
        return false;
    }
    info!("inventory full: {} is thrown down", item.base_name);
    let v = crate::item_drop::drop_velocity(gd_game_core::rng::u32(0..=0x7FFF));
    let e = spawn_thrown(
        commands,
        center,
        v,
        0.0,
        GroundItem {
            item: Some(item),
            gold: 0,
            yaw: 0.0,
        },
    );
    commands.entity(e).insert(crate::item_drop::DroppedByPlayer);
    false
}

/// Takes a ground item, then despawns it. False when the item is refused or there is no inventory
/// (it stays).
#[allow(clippy::too_many_arguments)]
fn take_ground_item(
    commands: &mut Commands,
    inv: &mut Option<ResMut<gd_game_core::inventory::Inventory>>,
    progress: &mut PlayerProgress,
    data: &GameData,
    cb: &Combatant,
    req: &gd_game_core::inventory::Requirements,
    ch: &gd_game_core::character::Character,
    tf: &Transform,
    player: Entity,
    texts: &mut MessageWriter<crate::floating::FloatingText>,
    picked: &mut MessageWriter<ItemPickedUp>,
    taken: &mut gd_game_core::world::ChestStates,
    e: Entity,
    gt: &GlobalTransform,
    gi: &GroundItem,
    placed: Option<&crate::lore::PlacedNote>,
) -> bool {
    if gi.gold > 0 {
        let gold = gold_with_bonus(gi.gold, cb.profile.sheet.gold_bonus);
        add_gold(&mut progress.gold, gold as i64);
        info!("picked up {gold} gold ({} total)", progress.gold);
        texts.write(crate::floating::FloatingText::money(
            &data.db,
            player,
            tf.translation,
            gold as u64,
        ));
    }
    if let Some(item) = &gi.item {
        let Some(inv) = inv.as_mut() else {
            return false;
        };
        if inv.refuses_pickup(item) {
            info!("{} cannot be picked up", item.base_name);
            return false;
        }
        // `Item::OnPickup`: written before placement.
        picked.write(ItemPickedUp {
            item: item.clone(),
            at: gt.translation(),
        });
        if let Some(t) =
            crate::floating::FloatingText::pickup(&data.db, player, tf.translation, &item.base_name)
        {
            texts.write(t);
        }
        let center = tf.translation + crate::monster_skills::character_center(data, ch);
        if give_item(
            commands,
            inv,
            data,
            Receiver { center, req },
            item.clone(),
            true,
        ) {
            info!("picked up {}", item.base_name);
            inv.mark_fresh(item);
        }
    }
    if let Some(p) = placed {
        taken.opened.insert(p.0);
    }
    commands.entity(e).try_despawn();
    true
}

/// Picking up. Auto-pickup is instant; a clicked item is taken through the pickup action: in reach the
/// player faces the item (`LookAtNoLean`), plays the Pickup slot and takes the item at its "Pickup"
/// animation event (`PickUpAction::Execute` / `AnimationCallback`); the action ends at "End". Reach
/// (`MoveAndPickUpItem`): player radius + item radius + 1.0, 3D.
#[allow(clippy::too_many_arguments)]
pub fn pick_up(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    #[cfg(feature = "dev")] mut scratch: Local<gd_sim::nav::PathScratch>,
    mut commands: Commands,
    mut progress: ResMut<PlayerProgress>,
    mut players: Query<
        (
            Entity,
            &Transform,
            &mut crate::player_orders::InteractTarget,
            &mut MovePath,
            &Combatant,
            &mut gd_game_core::character::Character,
            &gd_game_core::inventory::Requirements,
        ),
        With<Player>,
    >,
    items: Query<(
        Entity,
        &GlobalTransform,
        Ref<GroundItem>,
        Option<&crate::lore::PlacedNote>,
        Has<crate::item_drop::DroppedByPlayer>,
        Has<PickupDelay>,
    )>,
    waiting: Query<&Tossed>,
    #[cfg(feature = "dev")] nav: Res<Nav>,
    (mut inv, mut taken): (
        // Option: without an inventory the pickup is skipped (the item stays on the ground).
        Option<ResMut<gd_game_core::inventory::Inventory>>,
        ResMut<gd_game_core::world::ChestStates>,
    ),
    (data, mut texts, mut picked): (
        Res<GameData>,
        MessageWriter<crate::floating::FloatingText>,
        MessageWriter<ItemPickedUp>,
    ),
    (time, options, mut period): (
        Res<Time>,
        Res<gd_game_core::options::GameOptions>,
        Local<f32>,
    ),
    mut callbacks: MessageReader<gd_game_core::character::AnimCallback>,
) {
    let Ok((player, tf, mut interact, mut path, cb, mut ch, req)) = players.single_mut() else {
        return;
    };
    #[cfg(feature = "dev")]
    if hooks.items.auto_loot && cb.target.is_none() && interact.0.is_none_or(|e| !items.contains(e))
    {
        let near = items
            .iter()
            .filter(|(e, ..)| !waiting.get(*e).is_ok_and(|t| t.wait > 0.0))
            .map(|(e, gt, ..)| {
                (
                    e,
                    gt.translation(),
                    gt.translation().distance(tf.translation),
                )
            })
            .filter(|x| x.2 < 25.0)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((e, p, _)) = near
            && let Some(pts) = nav.0.find_path(
                &mut scratch,
                tf.translation.to_array(),
                p.to_array(),
                crate::nav::PATH_BUDGET_LONG,
            )
        {
            interact.0 = Some(e);
            path.points = pts.into_iter().skip(1).map(Vec3::from).collect();
        }
    }
    // `Player::UpdateSelf`: the auto-pickup scan every `kAutoPickupPeriod` (the first frame included).
    *period -= time.delta_secs();
    let scan = *period <= 0.0;
    if scan {
        *period = AUTO_PICKUP_PERIOD;
    }
    let loot_radius = options.float("autoLootRadius");
    // "Pickup" takes the clicked item, "End" finishes the action (a fallback take for a clip without
    // a "Pickup" point).
    let (mut pickup_event, mut pickup_end) = (false, false);
    for ev in callbacks.read() {
        if ev.entity == player && ev.anim == gd_sim::anim::AnimType::Pickup {
            pickup_event |= ev.name == "Pickup";
            pickup_end |= ev.name == "End";
        }
    }
    // The player's own drop is never auto-picked, nor an item in its [`PickupDelay`] (a new one
    // included: its delay is inserted this frame).
    let mut auto_taken = None;
    for (e, gt, gi, placed, own, delayed) in &items {
        if waiting.get(e).is_ok_and(|t| t.wait > 0.0) {
            continue;
        }
        let dist3 = gt.translation().distance(tf.translation);
        let auto = scan
            && !own
            && !delayed
            && !gi.is_added()
            && if gi.gold > 0 {
                dist3 < auto_pickup_radius(None, loot_radius)
            } else {
                gi.item
                    .as_ref()
                    .and_then(|i| data.db.get(&i.base_name))
                    .is_some_and(|r| {
                        dist3 < auto_pickup_radius(Some(r.class.as_str()), loot_radius)
                    })
            };
        if auto
            && take_ground_item(
                &mut commands,
                &mut inv,
                &mut progress,
                &data,
                cb,
                req,
                &ch,
                tf,
                player,
                &mut texts,
                &mut picked,
                &mut taken,
                e,
                gt,
                &gi,
                placed,
            )
            && interact.0 == Some(e)
        {
            auto_taken = Some(e);
        }
    }
    let Some(e) = interact.0 else { return };
    // Already taken by the auto pass this frame (despawn only queued).
    if auto_taken == Some(e) {
        interact.0 = None;
        path.points.clear();
        return;
    }
    // Another use system's target (an NPC, a chest, a door, …).
    let Ok((_, gt, gi, placed, ..)) = items.get(e) else {
        return;
    };
    if waiting.get(e).is_ok_and(|t| t.wait > 0.0) {
        return;
    }
    let item_radius = gi
        .item
        .as_ref()
        .and_then(|i| data.db.get(&i.base_name))
        .map_or(0.0, crate::loot_filter::item_radius);
    if gt.translation().distance(tf.translation) >= cb.radius + item_radius + 1.0 {
        return;
    }
    if gi
        .item
        .as_ref()
        .is_some_and(|it| inv.as_ref().is_some_and(|i| i.refuses_pickup(it)))
    {
        interact.0 = None;
        return;
    }
    let has_slot = ch.anims.has(gd_sim::anim::AnimType::Pickup);
    if !has_slot || pickup_event || pickup_end {
        // No Pickup slot (a form): take at once.
        if take_ground_item(
            &mut commands,
            &mut inv,
            &mut progress,
            &data,
            cb,
            req,
            &ch,
            tf,
            player,
            &mut texts,
            &mut picked,
            &mut taken,
            e,
            gt,
            &gi,
            placed,
        ) {
            interact.0 = None;
            path.points.clear();
        }
    } else if ch.state != gd_game_core::character::CharState::Pickup {
        // `WorldCoords::LookAtNoLean`, then the Pickup slot once.
        let dir = (gt.translation() - tf.translation).with_y(0.0);
        if dir.length_squared() > 1e-4 {
            ch.turn.face_now(dir);
        }
        ch.state = gd_game_core::character::CharState::Pickup;
        ch.playing = gd_sim::anim::AnimType::None;
        path.points.clear();
    }
}
