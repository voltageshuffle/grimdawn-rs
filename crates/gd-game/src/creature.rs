//! Creatures (monsters, NPCs, pets) as gameplay entities: which look a
//! record's variant gets (`Monster::SelectAlternativeMeshAndTextures`, worn
//! gear rolls) and its animation state. The game dresses the entity with
//! meshes when it sees the [`BodyRequest`].
//!
//! Scale (the root `Transform`'s, which the body, its animation speeds and
//! its marks follow): `Actor::Load` @0x180028c50 (Engine.dll) sets
//! the record's `scale`, then `Actor::Read` @0x18002cf30 reads the level
//! entity's extra data (`gd_formats::map::level::Entity::scale`) and calls
//! `SetScale(Vec3)` with it: a creature placed in a level takes the level's
//! scale, which replaces the record's (the Emissary, `npc_theemissary_02`:
//! record 1.15, placed 1.0, as tall as the player's 1.05; 9 stock placements
//! differ). A spawned one (proxy, script, skill) keeps the record's
//! ([`record_scale`]). PORT: the use radius (`combat::get_radius`; the
//! engine's `Actor::GetRadius` @0x18002b410 takes the current scale) stays the
//! record's for placed creatures: at most 15 % off on those 9.
use crate::character::{Character, character_for};
use crate::data::GameData;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gd_formats::msh::Mesh as Msh;
use gd_sim::equip::WeaponType;
use std::collections::HashMap;
use std::sync::Arc;

/// Equipment slot of an item record (`EquipManager::AttachItemAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    RightHand,
    LeftHand,
    Head,
    Chest,
    Shoulders,
    Hands,
    Legs,
    Feet,
}

pub fn slot_of(class: &str) -> Option<Slot> {
    Some(match class {
        "ArmorProtective_Head" => Slot::Head,
        "ArmorProtective_Chest" => Slot::Chest,
        "ArmorProtective_Shoulders" => Slot::Shoulders,
        "ArmorProtective_Hands" => Slot::Hands,
        "ArmorProtective_Legs" => Slot::Legs,
        "ArmorProtective_Feet" => Slot::Feet,
        "WeaponArmor_Shield" | "WeaponArmor_Offhand" => Slot::LeftHand,
        c if WeaponType::from_class(c).is_some() => Slot::RightHand,
        _ => return None,
    })
}

/// A creature's look: body mesh, the variant's base / bump texture, and the
/// item worn in each slot (default pieces, then the rolled gear).
#[derive(Clone, Debug, Default)]
pub struct CreatureLook {
    pub mesh: String,
    pub base_texture: Option<String>,
    pub bump_texture: Option<String>,
    pub items: HashMap<Slot, String>,
}

impl CreatureLook {
    /// The weapon types in the hands (`EquipManager::GetHandState`).
    pub fn weapons(
        &self,
        db: &gd_formats::vfs::Database,
    ) -> (Option<WeaponType>, Option<WeaponType>) {
        let w = |s: Slot| {
            self.items
                .get(&s)
                .and_then(|p| db.get(p))
                .and_then(|r| WeaponType::from_class(&r.class))
        };
        (w(Slot::RightHand), w(Slot::LeftHand))
    }
}

/// The look of `record`'s `variant` (`variant` stands in for the object id
/// that selects among `monsterMesh` / `monsterBaseTexture` and seeds the gear
/// rolls, so a creature looks the same each time it spawns).
pub fn creature_look(
    db: &Arc<gd_formats::vfs::Database>,
    record: &str,
    variant: u32,
) -> Option<CreatureLook> {
    let rec = db.get(record)?;
    let (mesh, base_texture, bump_texture) = look_mesh(rec, variant)?;
    let mut look = CreatureLook {
        mesh,
        base_texture,
        bump_texture,
        ..Default::default()
    };
    // Default pieces first (EquipManager::Load, as for the player): NPC
    // meshes are often just a head, the body comes from these.
    for (key, slot) in [
        ("defaultHeadPiece", Slot::Head),
        ("defaultShouldersPiece", Slot::Shoulders),
        ("defaultChestPiece", Slot::Chest),
        ("defaultLegsPiece", Slot::Legs),
        ("defaultFeetPiece", Slot::Feet),
        ("defaultHandsPiece", Slot::Hands),
    ] {
        if let Some(p) = rec.str(key) {
            look.items.insert(slot, p.to_owned());
        }
    }
    // Worn equipment: each slot's `chanceToEquip<Slot>` roll, then its
    // `loot<Slot>Item<n>` entry through the game's loot loader (item records
    // or loot tables) — NPC/monster bodies are mostly made of these pieces.
    let idb = gd_sim::items::ItemDb::new(db.clone());
    let mut rng = gd_sim::combat::Rng(variant.wrapping_mul(2_654_435_761).max(1) & 0x7FFF_FFFF);
    let params = gd_sim::items::LootParams::at_level(10);
    for (loc, s) in [
        (7usize, "RightHand"),
        (6, "LeftHand"),
        (0, "Head"),
        (2, "Chest"),
        (14, "Shoulders"),
        (5, "Hands"),
        (3, "Legs"),
        (4, "Feet"),
    ] {
        let chance = rec.f32(&format!("chanceToEquip{s}")).unwrap_or(0.0);
        if chance <= 0.0 || !gd_sim::items::loot::chance_roll(chance, &mut rng) {
            continue;
        }
        if let Some(roll) = gd_sim::items::loot::loot_loader(&idb, rec, loc, &params, &mut rng)
            && let Some(r) = db.get(&roll.base)
            && let Some(slot) = slot_of(&r.class)
        {
            look.items.insert(slot, roll.base);
        }
    }
    Some(look)
}

/// The body mesh of a creature of `record` (`variant`, [`creature_look`]): what its spawn parses
/// ([`BodyCache::ready`] lets a spawn wait for it).
pub fn creature_mesh(db: &gd_formats::vfs::Database, record: &str, variant: u32) -> Option<String> {
    look_mesh(db.get(record)?, variant).map(|(m, ..)| m)
}

/// `Monster::SelectAlternativeMeshAndTextures` (0x18037c650): the record's mesh and texture overrides for
/// `variant`.
fn look_mesh(
    rec: &gd_formats::arz::Record,
    variant: u32,
) -> Option<(String, Option<String>, Option<String>)> {
    let mesh = rec.str("mesh")?.to_owned();
    let (ms, bs, ns) = (
        rec.strs("monsterMesh"),
        rec.strs("monsterBaseTexture"),
        rec.strs("monsterBumpTexture"),
    );
    Some(if !ms.is_empty() {
        let i = variant as usize % ms.len();
        (
            ms[i].to_owned(),
            bs.get(i).map(|b| (*b).to_owned()),
            ns.get(i).map(|n| (*n).to_owned()),
        )
    } else if !bs.is_empty() {
        let i = variant as usize % bs.len();
        (
            mesh,
            Some(bs[i].to_owned()),
            ns.get(i).map(|n| (*n).to_owned()),
        )
    } else if !ns.is_empty() {
        (mesh, None, Some(ns[variant as usize % ns.len()].to_owned()))
    } else {
        (mesh, None, None)
    })
}

/// Parsed meshes (skeleton, attach points, hit boxes) by path: the one loader, the game's render models are
/// built from these too, so a character's body and its drawn mesh share one `Arc`. A mesh lives while a holder
/// does ([`crate::weak_cache`]). A spawn that can wait asks [`BodyCache::ready`] first: the mesh is then parsed in
/// the background ([`collect_body_loads`]) instead of on the game thread.
#[derive(Resource, Default)]
pub struct BodyCache {
    cache: crate::weak_cache::WeakCache<Msh>,
    /// Meshes parsed in the background ([`BodyCache::ready`]).
    loading: Vec<bevy::tasks::Task<(String, Option<Msh>)>>,
    /// The paths of `loading`, so a mesh is parsed once.
    in_flight: std::collections::HashSet<String>,
}

impl BodyCache {
    /// Cached meshes still in use (the memory audit).
    pub fn count(&self) -> usize {
        self.cache.live()
    }

    pub fn get(&mut self, data: &GameData, path: &str) -> Option<Arc<Msh>> {
        let key = gd_formats::vfs::normalized(path);
        self.cache.get_or_load(&key, || {
            data.vfs
                .read(&key)
                .and_then(|b| Msh::parse(&b))
                .inspect_err(|e| warn!("mesh {key}: {e:#}"))
                .ok()
        })
    }

    /// The mesh at `key` (normalized) if it is parsed, `Some(None)` if it failed; never loads.
    pub fn loaded(&self, key: &str) -> Option<Option<Arc<Msh>>> {
        self.cache.get(key)
    }

    /// Files a mesh parsed elsewhere (a background model load, gd-render `ModelCache::model_ready`) for `key`.
    /// Weak only: the model holds its mesh, so a warm slot here would only push creature meshes out.
    pub fn put(&mut self, key: &str, msh: &Arc<Msh>) {
        self.cache.insert(key, Some(msh));
    }

    /// Whether the mesh at `path` is parsed (or failed): [`BodyCache::get`] then costs nothing. Otherwise its parse
    /// starts in the background and this is false: the caller spawns on a later frame.
    pub fn ready(&mut self, data: &GameData, path: &str) -> bool {
        let key = gd_formats::vfs::normalized(path);
        if self.cache.get_kept(&key).is_some() {
            return true;
        }
        if self.in_flight.insert(key.to_string()) {
            let (key, data) = (key.into_owned(), data.clone());
            self.loading
                .push(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                    let msh = data
                        .vfs
                        .read(&key)
                        .and_then(|b| Msh::parse(&b))
                        .inspect_err(|e| warn!("mesh {key}: {e:#}"))
                        .ok();
                    (key, msh)
                }));
        }
        false
    }

    /// Forgets every mesh and pending parse (the mounted mod changed).
    pub fn clear(&mut self) {
        self.cache.clear();
        self.loading.clear();
        self.in_flight.clear();
    }
}

/// Files the meshes [`BodyCache::ready`] parsed in the background, kept warm until their spawns take them.
pub fn collect_body_loads(mut bodies: ResMut<BodyCache>) {
    use bevy::tasks::{block_on, futures_lite::future};
    let bodies = &mut *bodies;
    let mut i = 0;
    while i < bodies.loading.len() {
        let Some((key, msh)) = block_on(future::poll_once(&mut bodies.loading[i])) else {
            i += 1;
            continue;
        };
        drop(bodies.loading.swap_remove(i));
        bodies.in_flight.remove(&key);
        // A spawn that could not wait may have parsed it meanwhile: keep that one.
        if bodies.cache.get(&key).is_none() {
            bodies.cache.put(&key, msh);
        }
    }
}

/// The game dresses this creature with meshes for its look.
#[derive(Component, Clone, Debug)]
pub struct BodyRequest {
    pub record: String,
    pub look: CreatureLook,
}

/// The `scale` of a creature record (`Actor::Load`; missing or 0: 1).
pub fn record_scale(rec: &gd_formats::arz::Record) -> f32 {
    rec.f32("scale").filter(|s| *s > 0.0).unwrap_or(1.0)
}

/// Spawns a creature of `record` (`variant`, see [`creature_look`]) at
/// `transform` with its animation state, scaled by `placed` (a level
/// placement's scale, module doc) else by the record's; returns the entity
/// and its look.
pub fn spawn_creature(
    commands: &mut Commands,
    data: &GameData,
    bodies: &mut BodyCache,
    transform: Transform,
    record: &str,
    variant: u32,
    placed: Option<f32>,
) -> Option<(Entity, CreatureLook)> {
    let rec = data.db.get(record)?;
    let look = creature_look(&data.db, record, variant)?;
    let body = bodies.get(data, &look.mesh)?;
    let scale = placed.unwrap_or_else(|| record_scale(rec));
    let tf = transform.with_scale(Vec3::splat(scale));
    let e = commands
        .spawn((tf, GlobalTransform::from(tf), Name::new(record.to_owned())))
        .id();
    let (right, left) = look.weapons(&data.db);
    let character: Character = character_for(
        data,
        record,
        rec,
        body,
        right,
        left,
        false,
        scale,
        e.to_bits(),
    );
    let gear = crate::combat_sounds::Gear {
        right: look.items.get(&Slot::RightHand).cloned(),
        left: look.items.get(&Slot::LeftHand).cloned(),
        chest: look.items.get(&Slot::Chest).cloned(),
    };
    commands.entity(e).insert((
        character,
        BodyRequest {
            record: record.to_owned(),
            look: look.clone(),
        },
        gear,
    ));
    if rec.bool("hiddenFromCombat") {
        commands.entity(e).insert(crate::combat::HiddenFromCombat);
    }
    if rec.bool("invincible") {
        commands.entity(e).insert(crate::combat::Invincible);
    }
    if rec.str("description") == Some(crate::world::DUMMY_TAG) {
        commands.entity(e).insert(crate::world::Dummy);
    }
    // A ranged weapon in the right hand: the creature shoots.
    if let Some(r) = look
        .items
        .get(&Slot::RightHand)
        .and_then(|w| crate::casting::ranged_weapon(data, w))
    {
        commands.entity(e).insert(r);
    }
    Some((e, look))
}

/// What spawning creatures needs.
#[derive(SystemParam)]
pub struct Spawner<'w> {
    pub data: Res<'w, GameData>,
    pub bodies: ResMut<'w, BodyCache>,
}

/// How many monsters a deferred spawner ([`ready_to_spawn`]'s callers) spawns a frame at most.
pub const SPAWNS_PER_FRAME: usize = 8;

/// How long a deferred spawner works out new records' clip lists a frame ([`ready_to_spawn`]).
pub const CLIPS_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// Each record's clip paths by variant ([`crate::character::creature_clips`]): a deferred spawner's, worked out once a
/// session.
pub type ClipLists =
    std::collections::HashMap<String, std::collections::HashMap<u32, Arc<[String]>>>;

/// Whether a creature of `record` / `variant` spawns without loading on the game thread: its mesh and its animation
/// clips are parsed ([`BodyCache::ready`], [`crate::character::AnimCache::ready`]; each starts its background load
/// otherwise). Working out a record's clip list reads its animation table: past `deadline` a record whose list is
/// not known yet waits for a later frame.
pub fn ready_to_spawn(
    s: &mut Spawner,
    anims: &mut crate::character::AnimCache,
    lists: &mut ClipLists,
    record: &str,
    variant: u32,
    deadline: std::time::Instant,
) -> bool {
    let mut ready = true;
    if let Some(mesh) = creature_mesh(&s.data.db, record, variant) {
        ready &= s.bodies.ready(&s.data, &mesh);
    }
    let paths = match lists.get(record).and_then(|v| v.get(&variant)) {
        Some(p) => p.clone(),
        None if std::time::Instant::now() >= deadline => return false,
        None => {
            let p: Arc<[String]> =
                crate::character::creature_clips(&s.data, record, variant).into();
            lists
                .entry(record.to_owned())
                .or_default()
                .insert(variant, p.clone());
            p
        }
    };
    for p in paths.iter() {
        ready &= anims.ready(&s.data, p);
    }
    ready
}

/// [`spawn_creature`] through a [`Spawner`] at the record's scale; the entity only.
pub fn spawn(
    commands: &mut Commands,
    s: &mut Spawner,
    transform: Transform,
    record: &str,
    variant: u32,
) -> Option<Entity> {
    spawn_placed(commands, s, transform, record, variant, None)
}

/// [`spawn`] of a creature placed in a level at the placement's scale `placed` (module doc).
pub fn spawn_placed(
    commands: &mut Commands,
    s: &mut Spawner,
    transform: Transform,
    record: &str,
    variant: u32,
    placed: Option<f32>,
) -> Option<Entity> {
    spawn_creature(
        commands,
        &s.data,
        &mut s.bodies,
        transform,
        record,
        variant,
        placed,
    )
    .map(|x| x.0)
}
