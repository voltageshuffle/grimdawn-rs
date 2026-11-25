//! Creatures (monsters, NPCs, pets) as gameplay entities: which look a
//! record's variant gets (`Monster::SelectAlternativeMeshAndTextures`, worn
//! gear rolls) and its animation state. The game dresses the entity with
//! meshes when it sees the [`BodyRequest`].
//!
//! Scale (the root `Transform`'s, which the body, its animation speeds and
//! its marks follow): `Actor::Load` (Engine.dll) sets
//! the record's `scale`, then `Actor::Read` reads the level
//! entity's extra data (`gd_formats::map::level::Entity::scale`) and calls
//! `SetScale(Vec3)` with it: a creature placed in a level takes the level's
//! scale, which replaces the record's (the Emissary, `npc_theemissary_02`:
//! record 1.15, placed 1.0, as tall as the player's 1.05; 9 stock placements
//! differ). A spawned one (proxy, script, skill) keeps the record's
//! ([`record_scale`]). PORT: the use radius (`combat::get_radius`; the
//! engine's `Actor::GetRadius` takes the current scale) stays the
//! record's for placed creatures: at most 15 % off on those 9.
use crate::data::GameData;
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::msh::Mesh as Msh;
use gd_sim::equip::WeaponType;
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
    let mut rng = gd_rules::Rng(variant.wrapping_mul(2_654_435_761).max(1) & 0x7FFF_FFFF);
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

/// `Monster::SelectAlternativeMeshAndTextures`: the record's mesh and texture overrides for
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
/// the background (`gd_game::creature::collect_body_loads`) instead of on the game thread.
#[derive(Resource, Default)]
pub struct BodyCache {
    pub cache: crate::weak_cache::WeakCache<Msh>,
    /// Meshes parsed in the background ([`BodyCache::ready`]).
    pub loading: Vec<bevy::tasks::Task<(String, Option<Msh>)>>,
    /// The paths of `loading`, so a mesh is parsed once.
    pub in_flight: gd_formats::collections::HashSet<String>,
    /// Each body's toe bone indices ([`BodyCache::toe`]), by mesh address. The kept `Weak` pins the
    /// allocation, so a live entry's address cannot name another mesh.
    pub toes: HashMap<usize, (std::sync::Weak<Msh>, [Option<usize>; 2])>,
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

    /// The body's toe bone index (`GetFootCoords`: `BN_LToe` / `BN_RToe`, matched case-insensitively),
    /// resolved once per mesh instead of a bone-name scan per footstep. Dead entries (their mesh gone)
    /// are pruned when a new mesh resolves; [`Self::clear`] drops them all.
    pub fn toe(&mut self, body: &Arc<Msh>, left: bool) -> Option<usize> {
        let addr = Arc::as_ptr(body) as usize;
        if let Some((mesh, toes)) = self.toes.get(&addr)
            && mesh.strong_count() > 0
        {
            return toes[usize::from(!left)];
        }
        let find = |name: &str| {
            body.bones
                .iter()
                .position(|b| b.name.eq_ignore_ascii_case(name))
        };
        let toes = [find("BN_LToe"), find("BN_RToe")];
        self.toes.retain(|_, (mesh, _)| mesh.strong_count() > 0);
        self.toes.insert(addr, (Arc::downgrade(body), toes));
        toes[usize::from(!left)]
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
        self.toes.clear();
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
