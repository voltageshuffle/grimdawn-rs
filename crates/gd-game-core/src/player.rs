//! The player character's build and the values derived from it.
//!
//! Raw state: [`Loadout`] (worn gear, bio attributes, spent points, learned skills, active buffs) and
//! [`PlayerProgress`](crate::progress::PlayerProgress) (level, experience, unspent points, gold). Derived state,
//! each computed by one system and read by everyone else:
//! - [`EffectiveSkills`]: the skill book with the equipment's `+N` skill bonuses;
//! - the player's [`Combatant`] profile, rebuilt when marked [`ProfileDirty`].
//!
//! Item toggles (`Skill_BuffSelfToggled` / `…RadiusToggled` / `…AttackRadiusToggled` / `Skill_OnHitAttackRadius` as
//! an item's `itemSkillName`; Game.dll):
//! - an item's skills are created on equip (`SkillManager::AddItemSkills`) inactive, and `SkillManager::Update` runs
//!   `AutoActivateNow` on them for the main player: an off, enabled auto toggle with qualifying weapons and an
//!   `ExclusiveSkillCheck` switches on at no cost. So a worn item toggle is on unless stopped (death, a dispel, as
//!   learned ones) or its record is already on from another worn item (the port lists each record once,
//!   [`Loadout::item_toggles`]). No hot slot holds an auto toggle.
//! - while on it counts like a learned toggle (`SkillManager::GetCharAttributes` runs
//!   `CollectPassiveCharAttributes` over the item list too, [`Loadout::profile_with`]).
//! - unequipped: `RemoveItemSkills` starts the cooldown of an active one, `StopSkill`s and uninstalls it; it stops
//!   every other item skill it removes too, so an item-granted summon's pets die.
//! - the save keeps no item toggle state (`SkillManager::WriteProperties`), so a loaded one comes on again.
//! - a worn set's skill (`itemSkillName` on the `ItemSet` record) is an item skill too (`AddItemSkills` with the
//!   set's id); a piece coming off removes it.
//!
//! PORT: an item toggle's weapon requirements are not tested (no stock item toggle has any).
use crate::combat::Combatant;
use crate::data::GameData;
use bevy::prelude::*;
use gd_formats::save::gdc::Gdc;
use gd_formats::save::item::Item;
use gd_formats::vfs::Database;
use gd_sim::skills::{SkillBook, SkillTrees};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;

#[derive(Component)]
#[require(ControllerDirection, crate::inventory::Requirements)]
pub struct Player;

/// The keyboard movement's controller direction (`ControllerPlayer.controllerDirection`, horizontal, normalised) and
/// length (`ControllerPlayer.movementLength`: 1, or 1.414 for a diagonal; 0 with no key held), set every frame by
/// `InGameUI::Update`, and Keyboard Only's aim direction (`InGameUI.aimDirection`: the last direction a key gave,
/// kept when the keys are let go; zero before the first). Written by the game's input only.
#[derive(Component, Default, Clone, Copy, Debug, PartialEq)]
pub struct ControllerDirection {
    pub dir: Vec3,
    pub length: f32,
    pub aim: Vec3,
}

/// Marks the player for a profile rebuild from its `Loadout`; life keeps its fraction.
#[derive(Component)]
pub struct ProfileDirty;

/// `records/skills/default/` skills: always level 1 for the player.
pub fn is_default_skill(skill: &str) -> bool {
    const P: &[u8] = b"records/skills/default/";
    skill
        .as_bytes()
        .get(..P.len())
        .is_some_and(|h| h.eq_ignore_ascii_case(P))
}

pub fn player_record(female: bool) -> &'static str {
    if female {
        "records/creatures/pc/femalepc01.dbr"
    } else {
        "records/creatures/pc/malepc01.dbr"
    }
}

/// Splits a saved bio `[physique, cunning, spirit, life, energy]` into the base without the spent attribute points,
/// and those points: per attribute `(saved − initial) / increment` rounded down (at most 10 000), the initial values
/// being the pc record's `characterStrength` / `characterDexterity` / `characterIntelligence`. Adding the points
/// back gives the saved values again.
pub fn split_bio(
    db: &Database,
    levels: &gd_sim::stats::LevelTable,
    female: bool,
    saved: [f32; 5],
) -> ([f32; 5], gd_sim::stats::AllocatedPoints) {
    use gd_sim::attributes::CharAttr;
    let Some(initial) = db.get(player_record(female)).map(|r| {
        let b = gd_sim::stats::BioStore::from_record(r);
        [CharAttr::Physique, CharAttr::Cunning, CharAttr::Spirit].map(|a| b.base.get(a))
    }) else {
        return (saved, Default::default());
    };
    let [p, c, s, life, energy] = saved;
    let points = |v: f32, init: f32, inc: i32| {
        if inc > 0 {
            ((v - init) / inc as f32).floor().clamp(0.0, 10_000.0) as u32
        } else {
            0
        }
    };
    let a = gd_sim::stats::AllocatedPoints {
        physique: points(p, initial[0], levels.strength_increment),
        cunning: points(c, initial[1], levels.dexterity_increment),
        spirit: points(s, initial[2], levels.intelligence_increment),
    };
    let share = |n: u32, inc: i32| (n as i32 * inc) as f32;
    let base = [
        p - share(a.physique, levels.strength_increment),
        c - share(a.cunning, levels.dexterity_increment),
        s - share(a.spirit, levels.intelligence_increment),
        life - (a.physique as i32 * levels.life_increment
            + a.cunning as i32 * levels.life_increment_dexterity
            + a.spirit as i32 * levels.life_increment_intelligence) as f32,
        energy - share(a.spirit, levels.mana_increment),
    ];
    (base, a)
}

/// A new character's starting gear (item records) from the player record's `loot<Slot>Item1` with a non-zero
/// `chanceToEquip<Slot>`.
pub fn starting_gear(db: &Database, female: bool) -> Vec<String> {
    let rec = db.get(player_record(female));
    let mut out = Vec::new();
    if let Some(r) = rec {
        for s in [
            "RightHand",
            "LeftHand",
            "Head",
            "Chest",
            "Shoulders",
            "Hands",
            "Legs",
            "Feet",
        ] {
            if r.f32(&format!("chanceToEquip{s}")).unwrap_or(0.0) > 0.0
                && let Some(i) = r.str(&format!("loot{s}Item1"))
            {
                out.push(i.to_owned());
            }
        }
    }
    out
}

/// Equipped items of a save: armour slots plus the active weapon set.
pub fn equipped_items(gdc: &Gdc) -> Vec<Item> {
    let Some(inv) = &gdc.inventory.data else {
        return Vec::new();
    };
    let set = &inv.weapons[if inv.use_alternate != 0 { 1 } else { 0 }];
    inv.equipment
        .iter()
        .chain(set.slots.iter())
        .filter(|s| !s.item.is_empty())
        .map(|s| s.item.clone())
        .collect()
}

/// `EquipmentCtrl::GetEquippedLocation` of the save's 12 equipment slots, in `EquipmentCtrl::WriteProperties` order.
/// A weapon set's two slots are 9 and 10, the second set's also setting [`ALT_WEAPON_SET`].
const EQUIP_LOCATIONS: [u32; 12] = [1, 2, 3, 4, 5, 8, 6, 7, 12, 13, 14, 11];
/// `SkillManager::WriteProperties`.
pub const ALT_WEAPON_SET: u32 = 0x8000_0000;

/// The item `record` / `seed` as a save holds it: its record and location word (0 in a bag or stash page); none
/// once gone.
fn item_location<'a>(gdc: &'a Gdc, record: &str, seed: u32) -> Option<(&'a str, u32)> {
    let is = |it: &Item| it.seed == seed && it.base_name.eq_ignore_ascii_case(record);
    let inv = gdc.inventory.data.as_ref()?;
    let worn = inv
        .equipment
        .iter()
        .zip(EQUIP_LOCATIONS)
        .chain(inv.weapons[0].slots.iter().zip([9, 10]))
        .chain(
            inv.weapons[1]
                .slots
                .iter()
                .zip([9 | ALT_WEAPON_SET, 10 | ALT_WEAPON_SET]),
        )
        .map(|(s, l)| (&s.item, l));
    let packed = inv
        .sacks
        .iter()
        .flat_map(|s| s.items.iter().map(|i| (&i.item, 0)))
        .chain(
            gdc.stash
                .sacks
                .iter()
                .flat_map(|s| s.items.iter().map(|i| (&i.item, 0))),
        );
    worn.chain(packed)
        .find(|(it, _)| is(it))
        .map(|(it, l)| (it.base_name.as_str(), l))
}

/// An item skill instance (the engine's `Skill` in `SkillManager.itemSkills` / `itemSkillCache`): its record and
/// source.
pub type ItemSkillKey = (String, ItemSkillSource);

/// The worn items' and sets' item skills in the order the engine creates them on putting `items` on one by one
/// (`EquipmentCtrl::ReadProperties` → … → `ItemEquipment::AttachItem`): the item's own skills, then on the set's
/// count change its set skill (`ItemSet::SkillAugmentInstall` regrants it, so a set's skill follows its last piece),
/// then the component's and the augment's. PORT: within one item, the port's record order.
fn attach_order(db: &Database, items: &[Item]) -> Vec<ItemSkillKey> {
    let sets = gd_sim::items::sets::set_skills(db, items);
    let set_of = |it: &Item| {
        db.get(&it.base_name)
            .and_then(|r| r.str("itemSetName").map(gd_formats::vfs::normalize))
    };
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let own = |it: &Item| {
            gd_sim::skills::item_skill_grants(db, std::slice::from_ref(it))
                .into_iter()
                .filter(|g| g.item.is_some())
                .map(|g| g.skill)
                .collect::<Vec<_>>()
        };
        let all = own(it);
        let bare = own(&Item {
            relic_name: String::new(),
            relic_bonus: String::new(),
            augment_name: String::new(),
            ..it.clone()
        });
        let key = || ItemSkillSource::Item {
            record: it.base_name.to_ascii_lowercase(),
            seed: it.seed,
        };
        let pre = bare.len().min(all.len());
        out.extend(all[..pre].iter().map(|s| (s.clone(), key())));
        let set = set_of(it);
        let last = set.is_some() && !items[i + 1..].iter().any(|o| set_of(o) == set);
        if last {
            out.extend(
                sets.iter()
                    .filter(|s| Some(&s.set) == set.as_ref())
                    .map(|s| (s.skill.clone(), ItemSkillSource::Set(s.set.clone()))),
            );
        }
        out.extend(all[pre..].iter().map(|s| (s.clone(), key())));
    }
    out
}

/// The save's item skill list (`SkillManager::WriteProperties`): every item skill bound to a celestial power, the
/// worn ones (`worn`) then the cache's, each as skill record, power, controller, location word and the item's
/// record (bit 31 for the second weapon set), or else the set name (location 0). `bound` gives an instance's
/// (power, controller).
pub fn bound_item_skills(
    gdc: &Gdc,
    worn: &[ItemSkillKey],
    cache: &[CachedItemSkill],
    bound: impl Fn(&str, &ItemSkillSource) -> Option<(String, String)>,
) -> Vec<gd_formats::save::gdc::ItemSkill> {
    let cached = cache.iter().map(|c| (&c.grant.skill, &c.source));
    worn.iter()
        .map(|(s, src)| (s, src))
        .chain(cached)
        .filter_map(|(skill, src)| {
            let (autocast_skill, autocast_controller) = bound(skill, src)?;
            let (item_name, item_slot) = match src {
                ItemSkillSource::Item { record, seed } => item_location(gdc, record, *seed)
                    .map(|(r, l)| (r.to_owned(), l))
                    .unwrap_or_default(),
                ItemSkillSource::Set(set) => (set.clone(), 0),
            };
            Some(gd_formats::save::gdc::ItemSkill {
                skill_name: skill.clone(),
                autocast_skill,
                autocast_controller,
                item_slot,
                item_name,
            })
        })
        .collect()
}

/// The worn item skill a saved entry's binding comes back on (`SkillManager::ReadProperties`): a worn item's skill
/// of that record at that location and weapon set, or a set's skill of that set name. The cache is empty on load,
/// so a cached skill's binding is dropped.
pub fn item_skill_binding_restored(
    db: &Database,
    gdc: &Gdc,
    entry: &gd_formats::save::gdc::ItemSkill,
) -> Option<ItemSkillSource> {
    attach_order(db, &equipped_items(gdc))
        .into_iter()
        .find(|(skill, src)| {
            skill.eq_ignore_ascii_case(&entry.skill_name)
                && match src {
                    ItemSkillSource::Item { record, seed } => item_location(gdc, record, *seed)
                        .is_some_and(|(r, l)| {
                            l == entry.item_slot && r.eq_ignore_ascii_case(&entry.item_name)
                        }),
                    ItemSkillSource::Set(set) => set.eq_ignore_ascii_case(&entry.item_name),
                }
        })
        .map(|(_, src)| src)
}

/// The learned skills of a save (or an empty book for a new character), with the always-present potion skills.
pub fn skill_book_from_save(db: &Database, gdc: Option<&Gdc>) -> SkillBook {
    let mut skills = SkillBook {
        bonus: Default::default(),
        // Locked skills (`unk_205`) and potion containers' unlock levels (`sublevel`).
        locked: gdc
            .map(|g| {
                g.skills
                    .skills
                    .iter()
                    .filter(|s| s.unk_205 != 0)
                    .map(|s| s.name.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default(),
        unlock: gdc
            .map(|g| {
                g.skills
                    .skills
                    .iter()
                    .filter(|s| s.sublevel > 0)
                    .map(|s| (s.name.to_ascii_lowercase(), s.sublevel))
                    .collect()
            })
            .unwrap_or_default(),
        skills: gdc
            .map(|g| {
                g.skills
                    .skills
                    .iter()
                    .map(|s| (s.name.clone(), s.level))
                    .collect()
            })
            .unwrap_or_default(),
        // Toggles saved as off (`skill_active` 0).
        inactive: gdc
            .map(|g| {
                g.skills
                    .skills
                    .iter()
                    .filter(|s| {
                        s.skill_active == 0
                            && db
                                .get(&s.name)
                                .is_some_and(|r| gd_sim::skills::is_toggle(&r.class))
                    })
                    .map(|s| s.name.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default(),
        dispelled: Default::default(),
        gear_pet_mods: Vec::new(),
        item_grants: Vec::new(),
        ..Default::default()
    };
    gd_sim::potions::ensure_skills(db, &mut skills);
    skills.finish()
}

/// What the player's combat profile is built from: worn gear and the bio base values (save).
#[derive(Component, Clone)]
pub struct Loadout {
    pub female: bool,
    pub items: Vec<Item>,
    /// The bio base Physique / Cunning / Spirit / Life / Energy without the spent attribute points; None for a new
    /// character (the pc record's).
    pub bio: Option<[f32; 5]>,
    /// Every attribute point spent, the save's and this session's, on top of `bio`.
    pub allocated: gd_sim::stats::AllocatedPoints,
    /// The learned skills (points spent; no equipment bonuses).
    pub skills: SkillBook,
    /// The timed self buffs; a shapeshift form is an entry with no end.
    pub buffs: crate::skill_buffs::TimedBuffs,
    /// Skills granted for a while with their level (a shapeshift form's).
    pub granted: Vec<(String, u32)>,
    /// An active form: (its skill, `allowedSkillTypes`).
    pub form: Option<(String, Vec<String>)>,
    /// `SkillManager.itemSkillCache`: skills of items taken off this session, left at level 0.
    pub item_skill_cache: Vec<CachedItemSkill>,
    /// The worn items' and sets' item skills (`SkillManager.itemSkills`) in the order the engine created them: the
    /// load's [`attach_order`], then each skill put on later appended.
    pub worn_item_skills: Vec<ItemSkillKey>,
}

/// Where an item skill came from (what `RetrieveCachedItemSkill` matches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemSkillSource {
    /// A worn item, by its record and seed (the port's identity of the engine's item object).
    Item { record: String, seed: u32 },
    /// A worn set's skill, by the set record (`Skill::GetItemSetName`).
    Set(String),
}

/// An item skill cache entry (`SkillManager.itemSkillCache`): the skill at level 0 and its source.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedItemSkill {
    pub grant: gd_sim::skills::ItemSkillGrant,
    pub source: ItemSkillSource,
}

/// The worn items' and sets' skill grants with their sources.
pub fn sourced_grants(
    db: &Database,
    items: &[Item],
) -> Vec<(gd_sim::skills::ItemSkillGrant, ItemSkillSource)> {
    // `item_skill_grants` lists the sets' grants last, in `set_skills` order.
    let mut sets = gd_sim::items::sets::set_skills(db, items).into_iter();
    gd_sim::skills::item_skill_grants(db, items)
        .into_iter()
        .map(|g| {
            let source = match g.item.and_then(|i| items.get(i)) {
                Some(it) => ItemSkillSource::Item {
                    record: it.base_name.to_ascii_lowercase(),
                    seed: it.seed,
                },
                None => ItemSkillSource::Set(sets.next().map(|s| s.set).unwrap_or_default()),
            };
            (g, source)
        })
        .collect()
}

impl Loadout {
    /// A character's loadout from its save (or a new character's starting gear and empty book), with `skills` as
    /// learned. The save bio's spent attribute points are split off into [`Self::allocated`] ([`split_bio`]).
    pub fn from_save(
        db: &Database,
        levels: &gd_sim::stats::LevelTable,
        gdc: Option<&Gdc>,
        female: bool,
        skills: SkillBook,
    ) -> Self {
        let items = match gdc {
            Some(g) => equipped_items(g),
            None => starting_gear(db, female)
                .into_iter()
                .map(|r| Item {
                    base_name: r,
                    stack_count: 1,
                    ..Default::default()
                })
                .collect(),
        };
        let (bio, allocated) = match gdc {
            Some(g) => {
                let b = &g.bio;
                let (bio, allocated) = split_bio(
                    db,
                    levels,
                    female,
                    [b.physique, b.cunning, b.spirit, b.health, b.energy],
                );
                (Some(bio), allocated)
            }
            None => (None, Default::default()),
        };
        let worn_item_skills = attach_order(db, &items);
        Loadout {
            female,
            items,
            bio,
            allocated,
            skills,
            buffs: crate::skill_buffs::TimedBuffs::default(),
            granted: Vec::new(),
            form: None,
            item_skill_cache: Vec::new(),
            worn_item_skills,
        }
    }

    /// The item skill instance named `skill` a binding by name goes to: the first worn, else cached, one.
    pub fn item_skill_source(&self, skill: &str) -> Option<ItemSkillSource> {
        self.worn_item_skills
            .iter()
            .map(|(s, src)| (s, src))
            .chain(
                self.item_skill_cache
                    .iter()
                    .map(|c| (&c.grant.skill, &c.source)),
            )
            .find(|(s, _)| s.eq_ignore_ascii_case(skill))
            .map(|(_, src)| src.clone())
    }

    /// Puts on the worn items `items`. `SkillManager::RemoveItemSkills`: the skills of an item taken off are set to
    /// level 0 and, for a player that is not a copy, moved into the item skill cache. `AddItemSkills` first asks
    /// `RetrieveCachedItemSkill` for each skill of an item (or set) put on, which takes out the first cached skill of
    /// that name whose source matches: the same item coming back to the same location and weapon set, or, once its
    /// item is gone, the set name (only a set's skill matches). So a skill of another copy of the record stays
    /// cached. The worn ones keep their creation order: a skill taken off leaves it, one put on is appended in
    /// [`attach_order`], as is a set's skill regranted on a count change.
    pub fn set_items(&mut self, db: &Database, items: Vec<Item>) {
        let old = sourced_grants(db, &self.items);
        let new = sourced_grants(db, &items);
        let same = |a: &(gd_sim::skills::ItemSkillGrant, ItemSkillSource),
                    b: &(gd_sim::skills::ItemSkillGrant, ItemSkillSource)| {
            a.0.skill.eq_ignore_ascii_case(&b.0.skill) && a.1 == b.1
        };
        for g in new.iter().filter(|g| !old.iter().any(|o| same(o, g))) {
            if let Some(i) = self
                .item_skill_cache
                .iter()
                .position(|c| c.grant.skill.eq_ignore_ascii_case(&g.0.skill) && c.source == g.1)
            {
                self.item_skill_cache.remove(i);
            }
        }
        for (grant, source) in old
            .iter()
            .filter(|o| !new.iter().any(|g| same(o, g)))
            .cloned()
        {
            self.item_skill_cache.push(CachedItemSkill {
                grant: gd_sim::skills::ItemSkillGrant {
                    level: 0,
                    item: None,
                    ..grant
                },
                source,
            });
        }
        let regranted = gd_sim::items::sets::regranted_set_skills(
            &gd_sim::items::sets::worn_sets(db, &self.items),
            &gd_sim::items::sets::worn_sets(db, &items),
            &gd_sim::items::sets::set_skills(db, &items),
        );
        let kept: Vec<ItemSkillKey> = new
            .iter()
            .filter(|g| old.iter().any(|o| same(o, g)))
            .filter(|g| !matches!(g.1, ItemSkillSource::Set(_)) || !regranted.contains(&g.0.skill))
            .map(|g| (g.0.skill.clone(), g.1.clone()))
            .collect();
        self.worn_item_skills.retain(|k| kept.contains(k));
        for k in attach_order(db, &items) {
            if !self.worn_item_skills.contains(&k) {
                self.worn_item_skills.push(k);
            }
        }
        self.items = items;
    }

    /// The skill's active byte (`Skill.active`): set while a timed buff or a shapeshift form runs and for a learned
    /// or worn-item toggle that is switched on and not dispelled. The port keeps `buffs` / `skills.inactive` /
    /// `skills.dispelled` rather than a byte on each skill.
    pub fn skill_active(&self, db: &Database, skill: &str) -> bool {
        self.buffs.running(skill)
            || ((self.skills.level(skill) > 0 || self.grants_skill(db, skill))
                && !self.skills.switched_off(&skill.to_ascii_lowercase())
                && db
                    .get(skill)
                    .is_some_and(|r| gd_sim::skills::is_toggle(&r.class)))
    }

    /// The worn items' and sets' toggles that are not also learned, each once with its granted level, on or off.
    pub fn item_toggles(&self, db: &Database) -> Vec<(String, u32)> {
        let grants = gd_sim::skills::item_skill_grants(db, &self.items);
        let mut out: Vec<(String, u32)> = Vec::new();
        for g in &grants {
            if out.iter().any(|(o, _)| o.eq_ignore_ascii_case(&g.skill))
                || self.skills.level(&g.skill) > 0
                || !db
                    .get(&g.skill)
                    .is_some_and(|r| gd_sim::skills::is_toggle(&r.class))
            {
                continue;
            }
            out.push((
                g.skill.clone(),
                gd_sim::skills::granted_level(&grants, &g.skill),
            ));
        }
        out
    }

    /// The learned skills followed by the worn items' toggles: (record, level).
    pub fn skills_and_item_toggles(&self, db: &Database) -> Vec<(String, u32)> {
        let mut v = self.skills.skills.clone();
        v.extend(self.item_toggles(db));
        v
    }

    /// The character stats of the item toggles that are on (`SkillManager::GetCharAttributes` runs the item list's
    /// `CollectPassiveCharAttributes` like the learned ones'): self and radius toggles and `Skill_OnHitAttackRadius`
    /// as [`SkillBook::stat_sources`] takes the learned ones; a damage aura only its `character*` fields (PORT: that
    /// prefix is taken as its char and penalty stores' fields), its reserve among them.
    fn item_toggle_stats(
        &self,
        db: &Database,
        trees: &SkillTrees,
    ) -> Vec<(gd_formats::arz::Record, usize)> {
        let toggles = self.item_toggles(db);
        let book = SkillBook {
            skills: toggles
                .iter()
                .filter(|(s, _)| {
                    db.get(s)
                        .is_some_and(|r| r.class != "Skill_BuffAttackRadiusToggled")
                })
                .cloned()
                .collect(),
            inactive: self.skills.inactive.clone(),
            dispelled: self.skills.dispelled.clone(),
            ..Default::default()
        }
        .finish();
        let mut out = book.stat_sources(db, trees);
        for (s, level) in &toggles {
            let Some(mut r) = db
                .get_owned(s)
                .filter(|r| r.class == "Skill_BuffAttackRadiusToggled")
            else {
                continue;
            };
            if self.skills.switched_off(&s.to_ascii_lowercase()) {
                continue;
            }
            std::sync::Arc::make_mut(&mut r.fields).retain(|(k, _)| {
                k.to_ascii_lowercase().starts_with("character")
                    || k.eq_ignore_ascii_case("Class")
                    || k.eq_ignore_ascii_case("templateName")
            });
            out.push((r, *level as usize - 1));
        }
        out
    }

    /// The learned skills with the worn equipment's skill bonuses: the one definition of the player's effective
    /// skill levels.
    pub fn effective_skills(&self, db: &Database, trees: &SkillTrees) -> SkillBook {
        self.skills.with_item_bonuses(db, trees, &self.items)
    }

    /// The character's dual-wield mask: its learned skills, the worn items' and sets' skills (`SkillManager::Update`
    /// walks the item list too) and a form's granted skills.
    pub fn dual_mask(&self, db: &Database) -> u32 {
        let learned = self.skills.skills.iter().map(|(s, l)| (s.as_str(), *l));
        let items = gd_sim::skills::item_skill_grants(db, &self.items);
        let items = items.iter().map(|g| (g.skill.as_str(), g.level));
        let granted = self.granted.iter().map(|(s, l)| (s.as_str(), *l));
        gd_sim::skills::dual_mask(db, learned.chain(items).chain(granted))
    }

    /// Effective level of a skill for the player: the highest of the learned level with item bonuses, an
    /// item-granted skill's level and a form's granted level; default skills (`records/skills/default/`) are always
    /// level 1. `effective` is the player's current [`EffectiveSkills`].
    pub fn skill_level(&self, db: &Database, effective: &SkillBook, skill: &str) -> u32 {
        if is_default_skill(skill) {
            return 1;
        }
        let learned = effective.effective_level(db, skill);
        let granted = self
            .granted
            .iter()
            .filter(|(g, _)| g.eq_ignore_ascii_case(skill))
            .map(|g| g.1)
            .max()
            .unwrap_or(0);
        learned
            .max(gd_sim::skills::granted_level(&effective.item_grants, skill))
            .max(granted)
    }

    /// Whether a worn item or set grants `skill` ([`gd_sim::skills::item_skill_grants`]).
    fn grants_skill(&self, db: &Database, skill: &str) -> bool {
        gd_sim::skills::item_skill_grants(db, &self.items)
            .iter()
            .any(|g| g.skill.eq_ignore_ascii_case(skill))
    }

    /// An item skill (`Skill.isItemSkill`): a skill the worn gear grants that the book has not learned.
    /// `Skill::GetManaCost` takes its cost unreduced.
    pub fn is_item_skill(&self, db: &Database, skill: &str) -> bool {
        self.skills.level(skill) == 0 && self.grants_skill(db, skill)
    }

    /// Combat profile at `level` from the resolved equipment.
    pub fn profile(
        &self,
        data: &GameData,
        rules: &gd_sim::actor::Rules,
        level: i32,
    ) -> Option<gd_sim::actor::Profile> {
        self.profile_with(data, rules, level, None, None)
    }

    /// [`Loadout::profile`] plus the buffs allies' auras keep on the player and the charged
    /// `Skill_WeaponPool_ChargedScaling`s' passive part (`charges`).
    pub fn profile_with(
        &self,
        data: &GameData,
        rules: &gd_sim::actor::Rules,
        level: i32,
        auras: Option<&crate::skill_buffs::ReceivedBuffs>,
        charges: Option<&crate::weapon_pool::PoolCharges>,
    ) -> Option<gd_sim::actor::Profile> {
        use gd_sim::items::resolve;
        let idb = data.item_db();
        let items: Vec<gd_sim::items::ResolvedItem> =
            self.items.iter().filter_map(|i| resolve(&idb, i)).collect();
        // Item skill bonuses (+N to a skill / mastery) raise effective levels.
        let trees = data.skill_trees();
        let book = self.effective_skills(&data.db, &trees);
        let mut skills = book.stat_sources(&data.db, &trees);
        skills.extend(self.item_toggle_stats(&data.db, &trees));
        skills.extend(self.buffs.stat_records(&data.db));
        skills.extend(auras.map(|a| a.stat_records(&data.db)).unwrap_or_default());
        let charged = charges.map_or_else(Vec::new, |c| {
            Self::charged_passives(&data.db, &trees, &book, c)
        });
        for (_, recs, _) in &charged {
            skills.extend(recs.iter().cloned());
        }
        let mut p = gd_sim::actor::player_from_items(
            &data.db,
            &idb,
            rules,
            player_record(self.female),
            level.max(1),
            self.bio,
            self.allocated,
            &items,
            &skills,
        )?;
        p.active_costs = self.active_costs(&data.db, &trees, &book);
        p.charged = charged.into_iter().map(|(s, _, o)| (s, o)).collect();
        Some(p)
    }

    /// The learned `Skill_WeaponPool_ChargedScaling`s with charges (`charges`) of the effective `book`: (skill,
    /// passive records, their offensive modifiers).
    #[allow(clippy::type_complexity)]
    fn charged_passives(
        db: &Database,
        trees: &SkillTrees,
        book: &SkillBook,
        charges: &crate::weapon_pool::PoolCharges,
    ) -> Vec<(
        String,
        Vec<(gd_formats::arz::Record, usize)>,
        std::collections::BTreeMap<String, f32>,
    )> {
        let mut out = Vec::new();
        for (s, l) in &book.skills {
            if *l == 0
                || charges
                    .0
                    .get(&s.to_ascii_lowercase())
                    .is_none_or(|c| c.count == 0)
            {
                continue;
            }
            let Some(rec) = db
                .get(s)
                .filter(|r| r.class == "Skill_WeaponPool_ChargedScaling")
            else {
                continue;
            };
            let li = book.effective_level(db, s).max(1) as usize - 1;
            let mods: Vec<(gd_formats::arz::Record, usize)> = book
                .modifiers_of(db, trees, s)
                .into_iter()
                .filter_map(|(m, ml)| db.get_owned(&m).map(|r| (r, ml)))
                .collect();
            let (recs, off) = gd_sim::skills::charged_scaling_passive(rec, li, &mods);
            out.push((s.clone(), recs, off));
        }
        out
    }

    /// `SkillManager::GetActiveLifeCost` / `GetActiveManaCost`: [`gd_sim::actor::active_cost`] summed over the
    /// active `SkillActivatedBuffSelf` skills: the learned and item toggles that are on (at the effective level of
    /// `book`, with the learned modifiers) and the running timed buffs ([`Self::buffs`]). PORT: the gear skill
    /// modifiers are not summed (no stock modifier has an active cost).
    fn active_costs(&self, db: &Database, trees: &SkillTrees, book: &SkillBook) -> [f32; 2] {
        use gd_sim::actor::active_cost;
        let buff_self = |class: &str| gd_sim::skills::class_info(class).buff_self;
        let mut c = [0.0f32; 2];
        let mut add = |v: [f32; 2]| {
            c[0] += v[0];
            c[1] += v[1];
        };
        let toggles = book
            .skills
            .iter()
            .filter(|(_, l)| *l > 0)
            .map(|(s, _)| (s.clone(), book.effective_level(db, s)))
            .chain(self.item_toggles(db));
        for (s, level) in toggles {
            let low = s.to_ascii_lowercase();
            let Some(r) = db.get(&s) else { continue };
            if level == 0
                || !buff_self(&r.class)
                || !gd_sim::skills::is_toggle(&r.class)
                || self.skills.switched_off(&low)
            {
                continue;
            }
            let Some(prof) = gd_sim::skills::profile_record(db, &s) else {
                continue;
            };
            let mods: Vec<_> = book
                .modifiers_of(db, trees, &s)
                .into_iter()
                .filter_map(|(m, li)| db.get_owned(&m).map(|r| (r, li)))
                .collect();
            add(active_cost(&prof, level as usize - 1, &mods));
        }
        for (r, li) in self.buffs.stat_records(db) {
            // A running buff's stat record is its profile: its own record, a wrapper's buff record, or a learned
            // modifier.
            if buff_self(&r.class)
                || r.class.starts_with("SkillBuff_")
                || gd_sim::skills::is_modifier_class(&r.class)
            {
                add(active_cost(&r, li, &[]));
            }
        }
        c
    }
}

/// What spawning a player builds from the save (or a new character), shared by the game and the test harness.
pub struct SpawnData {
    pub female: bool,
    /// The worn items' look records.
    pub equipment: Vec<String>,
    pub bundle: (Player, Combatant, Loadout),
}

/// The player's skill book with equipment bonuses applied ([`Loadout::effective_skills`]): what every reader uses
/// as the player's skill levels.
#[derive(Component, Clone, Default, PartialEq, Deref)]
pub struct EffectiveSkills(pub SkillBook);

/// The worn items' toggles of the player (record, level), the worn sets' piece counts and every item / set skill
/// that is not also learned, kept to see one leave with its item or a set's count change.
#[derive(Component, Clone, Default, PartialEq, Debug)]
pub struct ItemToggles(
    pub Vec<(String, u32)>,
    pub Vec<(String, u32)>,
    pub Vec<String>,
);
