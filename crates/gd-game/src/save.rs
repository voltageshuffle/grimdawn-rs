//! Saving: `F5` writes the current character to the port's own save folder,
//! never the game's (`GD_SAVE_DIR`, default `save` in `gd_game_core::session::data_dir`,
//! laid out like the game's: `main/_<name>/player.gdc` (`user/_<name>/` for a character created in a custom
//! game: `Choice::player_folder`) and
//! `<map folder>/<Difficulty>/{quests,conversations}.gdd`, `map.fow` and `map.dat` (map path, mod, spawn:
//! [`write_map_dat`]), the map folder being
//! `levels_world001.map`, a Crucible arena's `maps_survivalworld_a.map` or a custom game's map:
//! `Choice::map_folder`). Load it
//! back with `GD_SAVE=<that player.gdc>`.
//!
//! The template is the loaded `player.gdc`, or for a new character
//! [`new_character`] (named by `GD_NAME`, default "Player"; [`NewCharacter`], rolled once per session, so
//! every save keeps its UID and seed); level,
//! experience, points, attributes (`Loadout::bio` plus every spent point, `Loadout::allocated`, baked in
//! like `BioStore::allocate`; the load splits them off again, `player::split_bio`), money, the loot filter options, equipment (both weapon sets and the active one), bags, the personal stash, skill levels and the skill refund count (`crate::reclamation`), discovered riftgates and respawn points, the lore codex (`lore.rs`), the unlocked tutorial pages (`tutorials.rs`),
//! the devotions, factions, play stats, the header's class name tag (`gd_sim::progress::class_tag`) and
//! the story state are replaced; the rest of the template is written back unchanged. The transfer stash goes to
//! `transfer.gst` in the session's shared folder (the save folder root; `<mod>/` in a custom game:
//! `Choice::shared_save_dir`), with the blacksmith's materials
//! (`reagents.gst`), the learned blueprints (`formulas.gst`, unchanged) and
//! the unlocked illusions (`transmutes.gst`, `illusion.rs`); a hardcore character's are the `.gsh` twins
//! (`Choice::shared_file`, `gd_formats::save::Shared`), so hardcore and softcore never share them.
//! When: F5, the menu's Exit / Quit, closing the window, a hardcore death, the shrine / potion / reclamation
//! windows, and the engine's autosave ([`AutoSave`]: 60 s of play after the inventory, riftgates, shrines, quests
//! or tokens changed; a new character at once). `GD_AUTOSAVE_SECS=<n>` (test hook) also saves every n seconds.
//!
//! Items outside every grid: the cursor item (`Inventory::held`) and the items in a window's item boxes
//! ([`BoxedItems`]: Inventor / Runekeeper slot, dismantle results). An item on the cursor or in a box
//! is in no sack (`CursorHandlerItemMove::Cancel` puts it back at its source cell of the bag /
//! stash / transfer page, else `GiveItemToPlayer`; the handler's destructor
//! `CursorHandlerItemMove::~CursorHandlerItemMove` gives nothing back);
//! Grim Dawn.exe `UIPlayerHud::CloseAllWindows` (the HUD's close-everything) calls the cursor handler's `Escape`
//! (-> `Cancel`) and hides every window, whose boxes go back through `ItemBox_ReturnItemToPlayer`. PORT: the
//! engine's quit / autosave path is not traced past that; here every save writes those items into the file
//! where the give-back's bag step would put them (main bag, selected bag, the others; then a personal stash
//! page), and the session's own state is left alone, so a save in mid-drag takes nothing off the cursor. An
//! item no grid has room for is written into the main bag's sack at cell 0,0 on top of what is there: the
//! next load finds it a cell or keeps it (`inventory::place_saved`, `Inventory::unplaced`, which is written
//! back the same way), so no save path loses an item.
//!
//! Shared files: one that could not be read is never written over (`stash::read_shared`, [`write_shared`]),
//! and its store takes no deposits that session (`stash` module doc).
//! The per-character map files (`quests.gdd`, `conversations.gdd`, `map.fow`) are guarded the same way
//! (`stash::read_guarded`): one the load could not read is set aside to `.bak` and skipped here, never
//! rewritten from the empty runtime.
//!
//! Order and failures: the shared files are written first, then `player.gdc`, `map.dat`, `map.fow` and the story
//! files. Each file is written on its own (write then rename): a failure is logged, the remaining files are still
//! written, and the save reports every file it could not write. The item stores (transfer stash, reagents) go
//! before the character, and when one of them fails the character's files are not written this time: the disk
//! keeps the last save's character beside the last save's store, so an item moved between the bags and that store
//! is never in neither file (a failed `player.gdc` after the stores leaves it in both, a duplicate). PORT: the
//! engine has no such rule; the next save tries again. The story
//! files are the current difficulty's `quests.gdd` / `conversations.gdd` and, when they changed, the other
//! difficulties' `quests.gdd` (their rewarded flags, set by a completion on a higher difficulty:
//! `gd_story::story::save`); a difficulty with no file gets one only when the story holds quest state of it.
use bevy::prelude::*;
use gd_formats::inventory::{Placed, Sack};
use gd_formats::save::Shared;
use gd_formats::save::gdc::{Gdc, InventorySack, SackItem, Skill};
use gd_formats::save::item::Item;
use gd_formats::save::map_dat::{MapDat, WorldCoords};
use gd_game_core::inventory::Inventory;
use gd_game_core::levels::{region_at, region_origin};
use gd_game_core::player::{Loadout, Player};
use gd_game_core::progress::PlayerProgress;
use gd_game_core::save::DIFFICULTY_DIRS;
use gd_game_core::save::MapLayout;
use gd_game_core::session::LoadedCharacter;
use gd_game_core::session::{LIVE_SAVES, SaveDir};
use std::borrow::Cow;

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveRequest>()
            .init_resource::<BoxedItems>()
            .init_resource::<gd_game_core::stash::SharedFiles>()
            .add_systems(Update, save_game.in_set(SaveStage::Save))
            // After the frame's gameplay, so it sees the frame's changes together (its request is saved the
            // next frame).
            .add_systems(PostUpdate, autosave.run_if(gd_game_core::session::in_game));
    }
}

/// The player's items that sit in a window's item boxes (module doc), kept by the one system that owns
/// those boxes (grimdawn `trade::boxed_items`); empty without such a window.
#[derive(Resource, Default)]
pub struct BoxedItems(pub Vec<Item>);

/// The first free cell for `size` in `sacks`, tried in `order`.
fn free_cell(
    sacks: &[Sack<Item>],
    order: impl Iterator<Item = usize>,
    size: gd_formats::inventory::Footprint,
) -> Option<(usize, (u32, u32))> {
    order
        .filter(|&b| b < sacks.len())
        .find_map(|b| sacks[b].find_position(size).map(|c| (b, c)))
}

/// Builds the character file from the template and the session state.
#[allow(clippy::too_many_arguments)]
fn build_gdc(
    template: &Gdc,
    progress: &PlayerProgress,
    loadout: &Loadout,
    inv: &Inventory,
    rules: &gd_sim::actor::Rules,
    travel: Option<&crate::riftgates::TravelState>,
    stash: Option<&gd_game_core::stash::Stash>,
    factions: Option<&crate::factions::PlayerFactions>,
    devotion: Option<&crate::devotion::DevotionState>,
    unlock: Option<usize>,
    db: &gd_formats::vfs::Database,
    boxed: &[Item],
) -> Gdc {
    let mut g = template.clone();
    if let Some(f) = factions {
        g.factions.factions = f.list.clone();
    }
    // The items outside every grid (module doc) find a cell in a copy of the bags, else of the stash pages.
    let mut bags = Cow::Borrowed(&inv.bags[..]);
    let mut pages = stash.map(|s| Cow::Borrowed(&s.pages[..]));
    let mut homeless: Vec<&Item> = Vec::new();
    for item in inv.held.iter().chain(boxed) {
        let size = inv.size_of(item);
        let placed = |x, y| Placed {
            item: item.clone(),
            x,
            y,
            size,
        };
        let sel = inv.bag_tab;
        let order = [0, sel].into_iter().chain(1..bags.len());
        if let Some((b, (x, y))) = free_cell(&bags, order, size) {
            bags.to_mut()[b].items.push(placed(x, y));
        } else if let Some(pages) = pages.as_mut()
            && let Some((p, (x, y))) = free_cell(pages, 0..pages.len(), size)
        {
            pages.to_mut()[p].items.push(placed(x, y));
        } else {
            error!(
                "save: no bag or stash page has room for {}: written on top of the main bag's first cell",
                item.base_name
            );
            homeless.push(item);
        }
    }
    if let (Some(s), Some(pages)) = (stash, &pages) {
        g.stash.sacks = gd_game_core::stash::to_sacks(
            &template.stash.sacks,
            pages,
            s.unplaced.iter().filter(|(t, _)| !t).map(|(_, u)| u),
        );
    }
    if let Some(t) = travel {
        use gd_formats::map::uid_bytes;
        let d = t.difficulty.min(2);
        g.teleports.lists[d] = t.teleports.iter().map(uid_bytes).collect();
        g.respawns.lists[d] = t.respawns.iter().map(uid_bytes).collect();
        g.respawns.current[d] = t.current.as_ref().map(uid_bytes).unwrap_or([0; 16]);
        for (od, u) in &t.unlocked {
            let (od, u) = ((*od).min(2), uid_bytes(u));
            if od != d && !g.teleports.lists[od].contains(&u) {
                g.teleports.lists[od].push(u);
            }
        }
    }
    let p = &progress.progress;
    g.bio.level = p.level.max(1) as u32;
    g.bio.experience = p.experience.min(u32::MAX as u64) as u32;
    g.bio.attribute_points = p.attribute_points.max(0) as u32;
    g.bio.skill_points = p.skill_points.max(0) as u32;
    g.header.level = g.bio.level;
    // Every spent attribute point goes into the bio (`BioStore::allocate`) on the base without them
    // (`Loadout::bio`, `player::split_bio`; a new character's is the template's).
    let l = &rules.levels;
    let a = loadout.allocated;
    let [physique, cunning, spirit, life, energy] = loadout.bio.unwrap_or([
        g.bio.physique,
        g.bio.cunning,
        g.bio.spirit,
        g.bio.health,
        g.bio.energy,
    ]);
    g.bio.physique = physique + (a.physique as i32 * l.strength_increment) as f32;
    g.bio.cunning = cunning + (a.cunning as i32 * l.dexterity_increment) as f32;
    g.bio.spirit = spirit + (a.spirit as i32 * l.intelligence_increment) as f32;
    g.bio.health = life
        + (a.physique as i32 * l.life_increment
            + a.cunning as i32 * l.life_increment_dexterity
            + a.spirit as i32 * l.life_increment_intelligence) as f32;
    g.bio.energy = energy + (a.spirit as i32 * l.mana_increment) as f32;
    g.info.money = progress.gold.min(u32::MAX as u64) as u32;
    // `Player::SaveNewFormatData` writes the current set (`GetAlternateConfig`).
    g.info.alternate_config = inv.alternate as u8;
    g.info.alternate_config_enabled = inv.alternate_enabled as u8;
    // A template whose inventory block has no data gets a fresh one, so the items are never dropped.
    if g.inventory.data.is_none() {
        g.inventory.data = new_character("", false, false, [0; 16], 0).inventory.data;
    }
    if let Some(d) = g.inventory.data.as_mut() {
        for (i, slot) in d.equipment.iter_mut().enumerate().take(12) {
            slot.item = inv.equipped[i].clone().unwrap_or_default();
            slot.attached = inv.is_attached(i) as u8;
        }
        // Both weapon sets (`crate::weapon_sets`): the inactive one flagged
        // and detached (`EquipmentCtrl::SetAlternateEquipment`);
        // a worn item failing its requirements is detached too (`inventory`).
        d.use_alternate = inv.alternate as u8;
        for (set, w) in d.weapons.iter_mut().enumerate() {
            let active = (set == 1) == inv.alternate;
            w.flag = (!active) as u8;
            for k in 0..2 {
                let s = &mut w.slots[k];
                s.item = if active {
                    inv.equipped[12 + k].clone()
                } else {
                    inv.other_weapons[k].clone()
                }
                .unwrap_or_default();
                s.attached = (active && inv.is_attached(12 + k)) as u8;
            }
        }
        // An extra bag the template lacks is a new sack with block id 0. Empty bags too: the load takes
        // the bag count from the sacks.
        while d.sacks.len() < bags.len() {
            d.sacks.push(InventorySack {
                block_id: 0,
                temp_bool: 0,
                items: Vec::new(),
            });
        }
        for (b, sack) in d.sacks.iter_mut().enumerate() {
            if let Some(bag) = bags.get(b) {
                sack.items = bag
                    .items
                    .iter()
                    .map(|p| SackItem {
                        item: p.item.clone(),
                        x: p.x,
                        y: p.y,
                    })
                    .collect();
            }
        }
        // What no grid holds (module doc): the load's unplaced items at their saved sack and cell, the
        // homeless ones on the main bag's first cell.
        let last = d.sacks.len().saturating_sub(1);
        let extra = inv
            .unplaced
            .iter()
            .map(|u| (u.sack.min(last), u.x, u.y, &u.item))
            .chain(homeless.iter().map(|item| (0, 0, 0, *item)));
        for (b, x, y, item) in extra {
            if let Some(sack) = d.sacks.get_mut(b) {
                sack.items.push(SackItem {
                    item: item.clone(),
                    x,
                    y,
                });
            }
        }
    }
    for (name, level) in &loadout.skills.skills {
        let active = !loadout.skills.inactive.contains(&name.to_ascii_lowercase());
        match g
            .skills
            .skills
            .iter_mut()
            .find(|s| s.name.eq_ignore_ascii_case(name))
        {
            Some(s) => {
                s.level = *level;
                let low = name.to_ascii_lowercase();
                s.unk_205 = loadout.skills.locked.contains(&low) as u8;
                if let Some(u) = loadout.skills.unlock.get(&low) {
                    s.sublevel = *u;
                }
                // Toggles: on / off (`skill_active`).
                if db
                    .get(name)
                    .is_some_and(|r| gd_sim::skills::is_toggle(&r.class))
                {
                    s.skill_active = active as u8;
                }
            }
            None => g.skills.skills.push(Skill {
                name: name.clone(),
                level: *level,
                enabled: 1,
                unk_205: loadout.skills.locked.contains(&name.to_ascii_lowercase()) as u8,
                devotion_level: 0,
                devotion_experience: 0,
                sublevel: loadout
                    .skills
                    .unlock
                    .get(&name.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
                skill_active: (active
                    && db
                        .get(name)
                        .is_some_and(|r| gd_sim::skills::is_toggle(&r.class)))
                    as u8,
                skill_transition: 0,
                autocast_skill: String::new(),
                autocast_controller: String::new(),
            }),
        }
    }
    // `Player::WriteNewFormatHeader`: every save writes `GetClassTag`.
    g.header.class_tag = gd_sim::progress::class_tag(db, &loadout.skills);
    if let Some(d) = devotion {
        write_devotion(&mut g, d, loadout);
    }
    if let Some(u) = unlock {
        // 3 reads as 0 (`CompleteDifficulty`).
        if g.info.greatest_difficulty == 3 || (g.info.greatest_difficulty as usize) < u {
            g.info.greatest_difficulty = u as u8;
        }
    }
    g
}

/// Devotion points, shrine lists, stars (removed ones dropped), celestial
/// power levels and bindings.
fn write_devotion(g: &mut Gdc, d: &crate::devotion::DevotionState, loadout: &Loadout) {
    g.bio.devotion_points = d.points;
    g.bio.total_devotion = d.total;
    g.skills.devotion_reclamation_points_used = d.reclaimed;
    g.shrines.lists[d.difficulty.min(2)] = crate::devotion::shrine_lists(d);
    g.skills
        .skills
        .retain(|s| d.data.locate(&s.name).is_none() || loadout.skills.level(&s.name) > 0);
    for s in &mut g.skills.skills {
        if let Some(&(level, xp)) = d.powers.get(&s.name.to_ascii_lowercase()) {
            s.devotion_level = level.max(1);
            s.devotion_experience = xp;
        } else if d.data.locate(&s.name).is_some() {
            s.devotion_level = s.devotion_level.max(1);
        }
        match d
            .bindings
            .iter()
            .find(|b| b.item.is_none() && b.skill.eq_ignore_ascii_case(&s.name))
        {
            Some(b) => {
                s.autocast_skill = b.power.clone();
                s.autocast_controller = b.controller.clone();
            }
            None => {
                s.autocast_skill.clear();
                s.autocast_controller.clear();
            }
        }
    }
    // An item skill instance's binding: the save's item skill list (`player::bound_item_skills`); a learned
    // skill's is on its entry above.
    let items = gd_game_core::player::bound_item_skills(
        g,
        &loadout.worn_item_skills,
        &loadout.item_skill_cache,
        |s, src| {
            d.bindings
                .iter()
                .find(|b| b.item.as_ref() == Some(src) && b.skill.eq_ignore_ascii_case(s))
                .map(|b| (b.power.clone(), b.controller.clone()))
        },
    );
    g.skills.item_skills = items;
}

/// The hotbar's slots go into the first bar (`PlayerHotSlotCtrl`): skills,
/// potion / evade / scroll options (`hotbar`); an unchanged skill or scroll
/// keeps its fields. A skill put on the bar is a plain skill slot
/// (`is_item_skill` 0, no item, location 0).
fn write_hotbar(g: &mut Gdc, db: &gd_formats::vfs::Database, set: i32, slots: &[Option<String>]) {
    use gd_formats::save::gdc::{HotSlot, HotSlotBar};
    let per = g.ui.slots_per_bar.max(slots.len() as u32) as usize;
    // One bar per skill set (`PlayerHotSlotCtrl::WriteProperties` writes
    // every page, keyed by its set id); the default set is the first bar.
    let i = match g.ui.bars.iter().position(|b| b.id == set as u32) {
        Some(i) => i,
        None if set == 0 && !g.ui.bars.is_empty() => 0,
        None => {
            g.ui.bars.push(HotSlotBar {
                id: set as u32,
                slots: Vec::new(),
            });
            g.ui.bars.len() - 1
        }
    };
    let bar = &mut g.ui.bars[i];
    while bar.slots.len() < per {
        bar.slots.push(HotSlot::Empty);
    }
    for (i, want) in slots.iter().enumerate() {
        let cur = &mut bar.slots[i];
        let Some(w) = want else {
            *cur = HotSlot::Empty;
            continue;
        };
        // The potion / evade / scroll options (`hotbar` module docs).
        let new = match crate::hotbar::option_kind(db, w) {
            crate::hotbar::OptionKind::HealthPotion => HotSlot::Plain(2),
            crate::hotbar::OptionKind::EnergyPotion => HotSlot::Plain(3),
            crate::hotbar::OptionKind::Evade => HotSlot::Plain(5),
            crate::hotbar::OptionKind::Scroll => {
                if matches!(&*cur, HotSlot::Scroll { a, .. } if a.eq_ignore_ascii_case(w)) {
                    continue;
                }
                let r = db.get(w);
                let s = |k: &str| r.as_ref().and_then(|r| r.str(k)).unwrap_or("").to_owned();
                HotSlot::Scroll {
                    a: w.clone(),
                    b: s("bitmapButtonUp"),
                    c: s("bitmapButtonDown"),
                    label: String::new(),
                }
            }
            crate::hotbar::OptionKind::Skill => {
                if matches!(&*cur, HotSlot::Skill { skill, .. } if skill.eq_ignore_ascii_case(w)) {
                    continue;
                }
                HotSlot::Skill {
                    skill: w.clone(),
                    is_item_skill: 0,
                    item: String::new(),
                    equip_location: 0,
                }
            }
        };
        *cur = new;
    }
}

/// `map.dat`'s `spawnCoords` (`Player::GetClosestRespawnPoint`): the
/// player's rotation, at the current respawn point (`respawn`, its world position) when it is
/// known, else at the player's own position; with the region it is in (None: no region, the
/// engine's `streamSpawnCoords` false).
pub fn spawn_coords(
    map: &gd_formats::map::MapFile,
    player: &Transform,
    respawn: Option<Vec3>,
) -> Option<WorldCoords> {
    let at = respawn.unwrap_or(player.translation);
    let i = region_at(map, at)?;
    let r = &map.regions[i];
    Some(WorldCoords {
        // Mat3 rows of the engine = bevy columns (as level entities, `map::level::Entity`).
        rotation: Mat3::from_quat(player.rotation).to_cols_array(),
        position: (at - region_origin(r)).to_array(),
        region: r.id,
    })
}

/// Where a `map.dat` spawn is in the world (`Game::LoadModMap`: used only when its region
/// is in the map, else the map's spawn point).
pub fn spawn_position(map: &gd_formats::map::MapFile, dat: &MapDat) -> Option<Vec3> {
    let s = dat.spawn.as_ref()?;
    let r = map.regions.iter().find(|r| r.id == s.region)?;
    Some(region_origin(r) + Vec3::from(s.position))
}

/// Where the player starts a session (None: a new character on a map without a spawn point, so no start yet).
/// `hook` is `GD_START` (`hooks.rs`): with x,z the y is taken from the ground once loaded; with x,y,z the ground
/// below y + 2 (undergrounds). `map_spawn` is the map's spawn point.
pub fn player_start(
    map: &gd_formats::map::MapFile,
    map_spawn: Option<Vec3>,
    hook: Option<&[f32]>,
    loaded: Option<&Gdc>,
    choice: &gd_game_core::session::Choice,
) -> Option<Vec3> {
    match hook {
        Some(&[x, z]) => return Some(Vec3::new(x, 100.0, z)),
        Some(&[x, y, z]) => return Some(Vec3::new(x, y + 2.0, z)),
        _ => {}
    }
    // New character: the map's spawn point.
    let Some(ch) = loaded else { return map_spawn };
    // `Game::LoadModMap`: the start is `map.dat`'s spawn when its region is in the map,
    // else the map's spawn point ([`spawn_position`]).
    let diff = ch.info.difficulty();
    if let Some(dat) = choice.read_map_dat(diff) {
        let p = spawn_position(map, &dat)
            .or(map_spawn)
            .unwrap_or(Vec3::ZERO);
        info!("player start {p:?} (map.dat)");
        return Some(p);
    }
    // PORT: a save without `map.dat` (the engine fails such a load; the port's saves before it
    // wrote one) starts at its current respawn point, else the map's spawn point. Game.dll
    // `Player::GetClosestRespawnPoint`: the point is the uid's instance-group entity data
    // (`InstanceGroups::entity_data`: its region's origin + its position), not a level entity.
    let want = gd_formats::map::uid_words(&ch.respawns.current[diff]);
    let found = map
        .instance_groups
        .entity_data(&want)
        .and_then(|e| e.world)
        .map(Vec3::from);
    let p = found.or(map_spawn).unwrap_or(Vec3::ZERO);
    info!(
        "player start {p:?} (respawn point {})",
        if found.is_some() {
            "found"
        } else {
            "missing, using map spawn"
        }
    );
    Some(p)
}

/// The session's `map.dat` (Grim Dawn.exe `Game::SaveCharacter`, every save: the map path, the
/// mod name and [`spawn_coords`]). PORT: without the map layout (headless) or a region the
/// spawn is left out, as the engine does for a position outside every region.
pub fn write_map_dat(
    choice: &gd_game_core::session::Choice,
    layout: Option<&MapLayout>,
    player: Option<&Transform>,
    respawn: Option<Vec3>,
) -> MapDat {
    let spawn = layout
        .zip(player)
        .and_then(|(l, t)| spawn_coords(&l.0, t, respawn));
    MapDat::new(choice.map_path(), choice.mod_name(), spawn)
}

pub(crate) fn write(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.to_string_lossy().contains(LIVE_SAVES),
        "refusing to write into the game's save folder: {}",
        path.display()
    );
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    // Write then rename, so a crash never leaves half a save.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Writes the account-wide `file` into the session's shared folder (`Choice::shared_file`), unless it
/// holds a file this session could not read and could not set aside (`stash::SharedFiles`): that one is
/// left as it is.
pub(crate) fn write_shared(
    shared: &gd_game_core::stash::SharedFiles,
    choice: &gd_game_core::session::Choice,
    save_dir: &SaveDir,
    file: Shared,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let path = choice.shared_file(save_dir, file);
    if !shared.writable(&path) {
        error!(
            "save: {} was unreadable at load and is not written",
            path.display()
        );
        return Ok(());
    }
    write(&path, bytes)
}

/// Save now (the game sends it for F5 and when the window closes).
#[derive(Message, Clone, Copy, Debug)]
pub struct SaveRequest;

/// A new character's file before anything of the session is in it ([`new_character`] with a UID and
/// seed rolled once by `session::begin`): every save of the session builds on it like a loaded
/// character's ([`LoadedCharacter`]), so the character keeps one identity. The engine creates the
/// player object once (`Game::CreatePlayer`, Grim Dawn.exe) and saves that same object every time.
#[derive(Resource)]
pub struct NewCharacter(pub Gdc);

impl NewCharacter {
    pub fn roll(choice: &gd_game_core::session::Choice) -> Self {
        let mut uid = [0u8; 16];
        uid.iter_mut().for_each(|b| *b = gd_game_core::rng::u8(..));
        let mut g = new_character(
            &choice.name,
            choice.female,
            choice.hardcore,
            uid,
            gd_game_core::rng::u32(..),
        );
        // A new character is in the main quest unless the session has a mod (`Game::CreatePlayer`:
        // `Player::SetIsInMainQuest(modName empty)`): a custom game's or a Crucible's character is not.
        g.info.is_in_main_quest = choice.mod_name().is_empty() as u8;
        Self(g)
    }
}

/// The menu's Create Character Accept (the Accept branch of the difficulty window's `DifficultyWindow::HandleButton`):
/// rolls the character ([`NewCharacter::roll`]), sets its difficulty byte exactly as [`crate::session::begin`]
/// does for a session, and writes `player.gdc` into the character's folder ([`gd_game_core::session::Choice::player_folder`])
/// with the same atomic [`write`] as every save. The player then starts it like any saved character.
/// An existing folder is refused: the menu's name check only sees characters whose file parses, so a
/// folder the scan missed must not be written over. Returns the written file's path and the character.
pub fn create_character(
    save_dir: &SaveDir,
    choice: &gd_game_core::session::Choice,
    difficulty: usize,
    veteran: bool,
) -> anyhow::Result<(std::path::PathBuf, Gdc)> {
    let mut g = NewCharacter::roll(choice).0;
    // The session's difficulty, as `session::begin` and `save_game` set it; no challenge in the Crucible.
    let mode = gd_rules::mode::Mode::new(difficulty, veteran && choice.survival.is_none());
    g.info.last_difficulty = gd_game_core::session::last_difficulty(mode, choice);
    let dir = choice.player_folder(save_dir, &g.header.name, g.info.is_in_main_quest != 0);
    anyhow::ensure!(!dir.exists(), "{} already exists", dir.display());
    let path = dir.join("player.gdc");
    g.encode().and_then(|b| write(&path, &b))?;
    Ok((path, g))
}

/// `Player::kAutoSavePeriod` (60000 ms, the `Player` constructor's countdown and `Player::AutoSave`'s
/// reset).
pub const AUTOSAVE_PERIOD: f32 = 60.0;

/// The player's autosave (`Player::UpdateSelf` / `Player::AutoSave`): a dirty flag and
/// a countdown ([`AUTOSAVE_PERIOD`]). The countdown runs only while the flag is set; at 0 it saves if autosave
/// is on, saving is enabled (`devotion::save_disabled` / `cauldron::save_disabled`) and the player is the main
/// one, then resets the countdown and clears the flag, saved or not. A manual save leaves both alone.
///
/// The flag's setters: an item given to or leaving the character, a riftgate / respawn point found, a devotion
/// shrine restored or discovered, a quest update or token, a dye, a rename or death reset. [`autosave`] watches
/// the port's state for those: the inventory contents (bags, worn items, both weapon sets and the cursor item,
/// hashed when `Inventory::version` moves; an item picked up and put back is no change), the riftgate / respawn
/// lists growing, the restored / discovered shrine lists growing, the story's token count growing and the
/// story's `ObjectiveSatisfied` / `QuestTaskCompleted` effects (`OnQuestUpdated`, game events 42 / 40). A dye
/// is used up, so the inventory change covers it. Map markers: no stock record is a `StaticMarker`, so none is
/// ever added.
///
/// A new character: the engine writes it at creation, and so does the port's menu ([`create_character`]).
/// PORT: a hook-started session (`GD_NAME` / `GD_SAVE`, `session::load_save`) still writes it on its first
/// frame with a player (`Player::ForceSave`: flag set, countdown 1 ms, [`AutoSave::created`]).
#[derive(Resource)]
pub struct AutoSave {
    pub dirty: bool,
    /// Seconds left while dirty.
    pub countdown: f32,
    /// What [`autosave`] saw last frame (inventory contents hash, riftgates + respawns, restored + discovered
    /// shrines; None while that state does not exist yet); None before its first look.
    seen: Option<[Option<u64>; 3]>,
    /// The `Inventory::version` the contents hash was taken at.
    version: u64,
    /// The story's token count last frame (None: no story yet; one appearing is no change).
    tokens: Option<usize>,
}

impl Default for AutoSave {
    fn default() -> Self {
        Self {
            dirty: false,
            countdown: AUTOSAVE_PERIOD,
            seen: None,
            version: 0,
            tokens: None,
        }
    }
}

impl AutoSave {
    /// A new character's session: saved on its first frame (`Player::ForceSave`: flag set,
    /// countdown 1 ms).
    pub fn created() -> Self {
        Self {
            dirty: true,
            countdown: 0.001,
            ..Self::default()
        }
    }
}

/// The inventory's contents (module doc of [`AutoSave`]).
fn contents_hash(inv: &Inventory) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    for (b, sack) in inv.bags.iter().enumerate() {
        for p in &sack.items {
            (b, p.x, p.y, &p.item).hash(&mut h);
        }
    }
    inv.equipped.hash(&mut h);
    inv.other_weapons.hash(&mut h);
    inv.held.hash(&mut h);
    h.finish()
}

/// [`AutoSave`]'s per-frame part of `Player::UpdateSelf`; runs while the player is in the game.
#[allow(clippy::too_many_arguments)]
pub fn autosave(
    time: Res<Time>,
    // Option: inserted by `session::begin`; absent in an app that starts no session.
    auto: Option<ResMut<AutoSave>>,
    inv: Res<Inventory>,
    // Option: created by their `init_*` systems once the session has a character.
    travel: Option<Res<crate::riftgates::TravelState>>,
    devotion: Option<Res<crate::devotion::DevotionState>>,
    shrine: Res<crate::devotion::ShrineWindow>,
    potions: Res<crate::cauldron::PotionWindow>,
    mut save: MessageWriter<SaveRequest>,
    mut quests: MessageReader<gd_game_core::story::StoryEffect>,
    // Option: a session without a story (an arena still loading) has no tokens yet.
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
) {
    let Some(mut auto) = auto else { return };
    let quest_updated = quests
        .read()
        .filter(|e| {
            matches!(
                e.0,
                gd_story::story::Effect::ObjectiveSatisfied { .. }
                    | gd_story::story::Effect::QuestTaskCompleted { .. }
            )
        })
        .count()
        > 0;
    let tokens = rt.map(|rt| rt.story.borrow().player.tokens.len());
    let token_added = matches!((auto.tokens, tokens), (Some(a), Some(b)) if b > a);
    auto.tokens = tokens;
    if quest_updated || token_added {
        auto.dirty = true;
    }
    let contents = match auto.seen {
        Some([c, ..]) if auto.version == inv.version => c,
        _ => Some(contents_hash(&inv)),
    };
    auto.version = inv.version;
    let now = [
        contents,
        travel
            .as_ref()
            .map(|t| (t.teleports.len() + t.respawns.len()) as u64),
        devotion
            .as_ref()
            .map(|d| (d.restored.len() + d.discovered.len()) as u64),
    ];
    // A list that appears (its `init_*` ran) is not a change.
    if let Some(seen) = auto.seen
        && seen
            .iter()
            .zip(&now)
            .any(|(a, b)| a.is_some() && b.is_some() && a != b)
    {
        auto.dirty = true;
    }
    auto.seen = Some(now);
    if !auto.dirty {
        return;
    }
    auto.countdown -= time.delta_secs();
    if auto.countdown > 0.0 {
        return;
    }
    if !crate::devotion::save_disabled(&shrine) && !crate::cauldron::save_disabled(&potions) {
        save.write(SaveRequest);
    }
    auto.countdown = AUTOSAVE_PERIOD;
    auto.dirty = false;
}

#[allow(clippy::too_many_arguments)]
pub fn save_game(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut requests: MessageReader<SaveRequest>,
    time: Res<Time>,
    mut since: Local<f32>,
    // Option: the save runs on a timer / request and writes whatever exists; the player-state ones are
    // checked together ("no player yet"), the others (stash, factions, fog, ...) are written only when present.
    loaded: Option<Res<LoadedCharacter>>,
    progress: Option<Res<PlayerProgress>>,
    inv: Option<Res<Inventory>>,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    players: Query<
        (
            &Loadout,
            Option<&gd_game_core::casting::Caster>,
            Option<&Transform>,
        ),
        With<Player>,
    >,
    story: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    travel: Option<Res<crate::riftgates::TravelState>>,
    stash: Option<Res<gd_game_core::stash::Stash>>,
    factions: Option<Res<crate::factions::PlayerFactions>>,
    devotion: Option<Res<crate::devotion::DevotionState>>,
    unlock: Option<Res<gd_game_core::session::DifficultyUnlock>>,
    (
        counts,
        died,
        data,
        fog,
        (choice, game_mode),
        illusions,
        (loot_filter, compass),
        save_dir,
        lore,
        tutorials,
        waypoints,
        layout,
        (shrine, potions, new_character),
        reclaim,
        boxed,
        shared,
    ): (
        Res<crate::play_stats::PlayCounts>,
        Query<(), (With<Player>, Added<gd_game_core::combat::Dead>)>,
        Res<gd_game_core::data::GameData>,
        Option<Res<crate::fog::FogOfWar>>,
        (
            Res<gd_game_core::session::Choice>,
            Res<gd_game_core::session::GameMode>,
        ),
        Option<Res<crate::illusion::Illusions>>,
        (
            Option<Res<crate::loot_filter::LootFilter>>,
            Option<Res<crate::compass::CompassMode>>,
        ),
        Res<SaveDir>,
        Option<Res<crate::lore::LoreCodex>>,
        Option<Res<crate::tutorials::TutorialPages>>,
        Option<Res<crate::riftgates::WaypointIndex>>,
        Option<Res<MapLayout>>,
        (
            Res<crate::devotion::ShrineWindow>,
            Res<crate::cauldron::PotionWindow>,
            Option<Res<NewCharacter>>,
        ),
        Option<Res<crate::reclamation::SkillReclamation>>,
        Res<BoxedItems>,
        Res<gd_game_core::stash::SharedFiles>,
    ),
) {
    *since += time.delta_secs();
    // The timed autosave waits while a shrine or potions window has saving disabled.
    let auto = hooks.session.autosave_secs.is_some_and(|s| *since >= s)
        && !crate::devotion::save_disabled(&shrine)
        && !crate::cauldron::save_disabled(&potions);

    // F5, and closing the window (the game saves on quit).
    let requested = requests.read().count() > 0;
    // A hardcore death is written at once.
    let hardcore_death =
        !died.is_empty() && gd_game_core::session::is_hardcore(loaded.as_deref(), &choice);
    if !requested && !auto && !hardcore_death {
        return;
    }
    *since = 0.0;
    let (Some(progress), Some(inv), Some(rules), Ok((loadout, caster, player_at))) =
        (progress, inv, rules, players.single())
    else {
        warn!("save: no player yet");
        return;
    };
    // The session's character: the loaded file or the new one (`NewCharacter`, one UID / seed per session).
    let template = match (&loaded, &new_character) {
        (Some(l), _) => l.0.as_ref(),
        (None, Some(n)) => &n.0,
        (None, None) => {
            warn!("save: no session character");
            return;
        }
    };
    let mut g = build_gdc(
        template,
        &progress,
        loadout,
        &inv,
        &rules.0,
        travel.as_deref(),
        stash.as_deref(),
        factions.as_deref(),
        devotion.as_deref(),
        unlock.map(|u| u.0),
        &data.db,
        &boxed.0,
    );
    // The session's difficulty, challenge and game mode, for a new character as for a loaded one.
    g.info.last_difficulty = gd_game_core::session::last_difficulty(game_mode.0, &choice);
    if let Some(c) = caster {
        // Mouse slots back in set order (`weapon_sets::save_order`).
        for (set, page) in c.pages() {
            write_hotbar(
                &mut g,
                &data.db,
                set,
                &crate::weapon_sets::save_order(page, inv.alternate),
            );
        }
    }
    if let Some(r) = &reclaim {
        g.skills.skill_reclamation_points_used = r.used;
    }
    if let Some(l) = &lore {
        g.notes.notes = l.notes.clone();
    }
    if let Some(t) = &tutorials {
        g.tutorials.pages = t.pages.clone();
    }
    counts.apply(&mut g.stats);
    // The Crucible's records: greatest difficulty completed, greatest wave tier and score, defenses built and
    // power-ups activated.
    if let Some(rt) = &story {
        let mut st = rt.story.borrow_mut();
        st.game.survival.merge_records();
        let sv = &st.game.survival;
        g.info.greatest_survival_difficulty = sv.greatest_difficulty;
        g.stats.survival_greatest_wave = sv.greatest_wave;
        g.stats.survival_greatest_score = sv.greatest_score;
        g.stats.survival_defenses_built = sv.defenses_built;
        g.stats.survival_powerups_activated = sv.power_ups;
        g.info.current_tribute = st.player.tribute;
        g.stats.difficulty_skip = st.player.difficulty_skip;
        // A Crucible session's tokens are the character's survival vector of this difficulty.
        if choice.survival.is_some() {
            let d = g.info.difficulty();
            crate::survival::store_tokens(&mut g, d, &st.player.tokens);
        }
    }
    if let Some(f) = &loot_filter {
        g.info.loot_filters = f.to_save();
        g.info.loot_filter_count = g.info.loot_filters.len() as u32;
    }
    if let Some(c) = &compass {
        g.info.compass_state = c.0;
    }
    let dir = choice.player_folder(&save_dir, &g.header.name, g.info.is_in_main_quest != 0);
    // Each file is written on its own (module doc): a failure is logged and the others still go out.
    let mut failed: Vec<String> = Vec::new();
    let mut attempt = |what: &str, r: anyhow::Result<()>| match r {
        Ok(()) => true,
        Err(e) => {
            error!("save: {what}: {e:#}");
            failed.push(what.to_owned());
            false
        }
    };
    // The item stores: a failed one keeps the character's files at the last save (module doc).
    let mut stores_ok = true;
    if let Some(s) = &stash {
        let mut t = s.transfer_file.clone();
        t.sacks = gd_game_core::stash::to_sacks(
            &s.transfer_file.sacks,
            &s.transfer,
            s.unplaced.iter().filter(|(t, _)| *t).map(|(_, u)| u),
        );
        // `GameEngine::WritePlayerTransfer`: every write names the session's mod
        // (none in the Crucible, mode 1) and the loaded expansions.
        t.mod_name = match choice.game_mode() {
            1 => String::new(),
            _ => choice.mod_name().to_owned(),
        };
        t.expansion_status = gd_game_core::stash::LOADED_EXPANSIONS;
        stores_ok &= attempt(
            "transfer stash",
            t.encode()
                .and_then(|b| write_shared(&shared, &choice, &save_dir, Shared::Transfer, &b)),
        );
        let mut m = s.materials.clone();
        m.count = m.entries.len() as u32;
        stores_ok &= attempt(
            "reagents",
            m.encode()
                .and_then(|b| write_shared(&shared, &choice, &save_dir, Shared::Reagents, &b)),
        );
        attempt(
            "formulas",
            s.formulas
                .encode()
                .and_then(|b| write_shared(&shared, &choice, &save_dir, Shared::Formulas, &b)),
        );
    }
    if let Some(i) = &illusions {
        attempt(
            "transmutes",
            i.file
                .encode()
                .and_then(|b| write_shared(&shared, &choice, &save_dir, Shared::Transmutes, &b)),
        );
    }
    if !stores_ok {
        error!(
            "save failed: an item store was not written, so the character's files keep the last save"
        );
        return;
    }
    attempt(
        "player.gdc",
        g.encode().and_then(|b| write(&dir.join("player.gdc"), &b)),
    );
    let map_folder = dir.join(choice.map_folder());
    // `map.dat` beside the quests, for the current difficulty. PORT: the current respawn
    // point's position comes from the map-wide waypoint index (the player's own position
    // until that index is built).
    let respawn = travel
        .as_ref()
        .and_then(|t| t.current)
        .and_then(|u| waypoints.as_ref()?.0.get(&u).map(|p| p.pos));
    let map_dat = write_map_dat(&choice, layout.as_deref(), player_at, respawn);
    attempt(
        "map.dat",
        map_dat.encode().and_then(|b| {
            write(
                &map_folder
                    .join(DIFFICULTY_DIRS[g.info.difficulty()])
                    .join("map.dat"),
                &b,
            )
        }),
    );
    if let Some(f) = &fog {
        let out = map_folder.join(crate::fog::FogOfWar::dir_name(f.difficulty));
        let path = out.join("map.fow");
        if shared.writable(&path) {
            attempt(
                "map.fow",
                f.to_fow().encode().and_then(|b| write(&path, &b)),
            );
        } else {
            error!(
                "save: {} was unreadable at load and is not written",
                path.display()
            );
        }
    }
    if let Some(rt) = &story {
        let s = rt.story.borrow();
        let current = s.difficulty();
        let src_folder = choice.save_file(choice.map_folder());
        for (d, name) in DIFFICULTY_DIRS.iter().enumerate() {
            let read = |f: &str| {
                src_folder
                    .as_ref()
                    .and_then(|p| std::fs::read(p.join(name).join(f)).ok())
            };
            let q_tpl =
                read("quests.gdd").and_then(|b| gd_formats::save::gdd::Quests::parse(&b).ok());
            // Another difficulty's file (module doc): only where the character has one or holds state of it.
            if d != current && q_tpl.is_none() && !s.has_quest_state(d) {
                continue;
            }
            let out = map_folder.join(name);
            let mut quests = s.save_quests(d, q_tpl.as_ref());
            // PORT: the arena's `quests.gdd` carries no tokens (they are in `player.gdc`, shared by the arenas).
            if choice.survival.is_some() {
                quests.tokens.clear();
            }
            let path = out.join("quests.gdd");
            if shared.writable(&path) {
                attempt(
                    &format!("{name}/quests.gdd"),
                    quests.encode().and_then(|b| {
                        // Another difficulty's file is rewritten only when it changed.
                        if d != current && std::fs::read(&path).is_ok_and(|old| old == b) {
                            return Ok(());
                        }
                        write(&path, &b)
                    }),
                );
            } else {
                error!(
                    "save: {} was unreadable at load and is not written",
                    path.display()
                );
            }
            if d == current {
                let c_tpl = read("conversations.gdd")
                    .and_then(|b| gd_formats::save::gdd::Conversations::parse(&b).ok());
                let path = out.join("conversations.gdd");
                if shared.writable(&path) {
                    attempt(
                        &format!("{name}/conversations.gdd"),
                        s.save_conversations(c_tpl.as_ref())
                            .encode()
                            .and_then(|b| write(&path, &b)),
                    );
                } else {
                    error!(
                        "save: {} was unreadable at load and is not written",
                        path.display()
                    );
                }
            }
        }
    }
    if failed.is_empty() {
        info!("saved {}", dir.join("player.gdc").display());
    } else {
        error!("save failed: {} not written", failed.join(", "));
    }
}

/// The save write: what must be in the file orders before `Save`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveStage {
    Save,
}

/// Hotbar 0 of a new character: `(slot, type)` with type 0 = default weapon attack,
/// otherwise a `HotSlot::Plain` type (2/3 health/mana potion, 5 evade).
const NEW_HOTBAR: [(usize, u32); 6] = [(10, 0), (11, 0), (24, 2), (25, 3), (26, 0), (46, 5)];

/// Hotbar 0 of a new character ([`NEW_HOTBAR`]): the default weapon attack
/// on both weapon sets' left mouse slots (10, 11) and slot 26.
pub fn new_hotbar() -> Vec<gd_formats::save::gdc::HotSlot> {
    use gd_formats::save::gdc::HotSlot;
    let mut hotbar = vec![HotSlot::Empty; 47];
    for (i, ty) in NEW_HOTBAR {
        hotbar[i] = if ty == 0 {
            HotSlot::Skill {
                skill: "records/skills/default/defaultweaponattack.dbr".into(),
                is_item_skill: 0,
                item: String::new(),
                equip_location: 0,
            }
        } else {
            HotSlot::Plain(ty)
        };
    }
    hotbar
}

/// A level-1 character as the game writes it after creating it and loading it once (reference: a real character
/// created, entered and quit without moving; field-for-field test in `tests/game/new_character.rs`). Differences:
/// - The starting armour/weapon are rolled at random on first load (the reference's differ from those of another
///   fresh character); the port's inventory replaces them when saving. Item seeds here derive from `seed`.
/// - The starting necklace is given by the story on first load (`GiveItem`), so it is not part of this.
/// - `sex` is 1 for male, 0 for female. `texture` is the record's `playerTextures` entry
///   (`Player::PickNewTexture`); `femalepc01.dbr` has none, so a female keeps the constructor's empty string
///   (`Player::Player` / `PickNewTexture`).
pub fn new_character(
    name: &str,
    female: bool,
    hardcore: bool,
    uid: gd_formats::save::gdc::Uid,
    seed: u32,
) -> Gdc {
    use gd_formats::save::gdc::*;
    use gd_formats::save::item::StashSack;
    let mut rng = seed ^ 0x9E37_79B9;
    let mut roll = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let gear = |roll: &mut dyn FnMut() -> u32, base: &str| EquipSlot {
        item: Item {
            base_name: base.into(),
            seed: roll(),
            stack_count: 1,
            ..Item::default()
        },
        attached: 1,
    };
    let empty = |seed: u32| EquipSlot {
        item: Item {
            seed,
            stack_count: 1,
            ..Item::default()
        },
        attached: 0,
    };

    let chest = gear(&mut roll, "records/items/geartorso/a00_torso003.dbr");
    let legs = gear(&mut roll, "records/items/gearlegs/a00_legs01.dbr");
    let legs_seed = legs.item.seed;
    let mut equipment = vec![empty(0), empty(0), chest, legs];
    equipment.extend((0..8).map(|_| empty(legs_seed)));
    let weapons = [
        WeaponSet {
            flag: 0,
            slots: [
                gear(
                    &mut roll,
                    "records/items/gearweapons/blunt1h/a00_blunt001.dbr",
                ),
                gear(
                    &mut roll,
                    "records/items/gearweapons/shields/a00_shield01.dbr",
                ),
            ],
        },
        WeaponSet {
            flag: 1,
            slots: [empty(0), empty(0)],
        },
    ];

    let per_diff = || PerDifficultyStats {
        greatest_monster_killed_name: String::new(),
        greatest_monster_killed_level: 0,
        greatest_monster_killed_life_and_mana: 0,
        last_monster_hit: String::new(),
        last_monster_hit_by: String::new(),
    };
    let hotbar = new_hotbar();

    Gdc {
        seed,
        header: Header {
            version: 2,
            name: name.into(),
            sex: u8::from(!female),
            class_tag: String::new(),
            level: 1,
            hardcore: u8::from(hardcore),
            expansion_status: 7,
        },
        data_version: 8,
        uid,
        info: CharacterInfo {
            version: 5,
            is_in_main_quest: 1,
            has_been_in_game: 1,
            last_difficulty: 0,
            greatest_difficulty: 0,
            money: 0,
            greatest_survival_difficulty: 0,
            current_tribute: 0,
            compass_state: 3,
            skill_window_show_help: 1,
            alternate_config: 0,
            alternate_config_enabled: 0,
            texture: if female {
                ""
            } else {
                "creatures/pc/hero02.tex"
            }
            .into(),
            loot_filter_count: 42,
            loot_filters: NEW_LOOT_FILTERS.to_vec(),
        },
        bio: Bio {
            version: 8,
            level: 1,
            experience: 0,
            attribute_points: 0,
            skill_points: 0,
            devotion_points: 0,
            total_devotion: 0,
            physique: 50.0,
            cunning: 50.0,
            spirit: 50.0,
            health: 250.0,
            energy: 250.0,
        },
        inventory: Inventory {
            version: 11,
            data: Some(InventoryData {
                focused: 0,
                selected: 0,
                sacks: vec![InventorySack {
                    block_id: 0,
                    temp_bool: 0,
                    items: Vec::new(),
                }],
                use_alternate: 0,
                equipment,
                weapons,
            }),
        },
        stash: Stash {
            version: 11,
            sacks: vec![StashSack {
                block_id: 0,
                width: 0,
                height: 0,
                items: Vec::new(),
                unk: [0; 4],
                name: String::new(),
            }],
        },
        respawns: Respawns {
            version: 1,
            lists: Default::default(),
            current: [[0; 16]; 3],
        },
        teleports: UidLists {
            version: 1,
            lists: Default::default(),
        },
        markers: UidLists {
            version: 1,
            lists: Default::default(),
        },
        shrines: Shrines {
            version: 2,
            lists: [
                [Vec::new(), Vec::new()],
                [Vec::new(), Vec::new()],
                [Vec::new(), Vec::new()],
            ],
        },
        skills: Skills {
            version: 8,
            skills: NEW_SKILLS
                .iter()
                .map(|&(name, level, enabled, unk_205)| Skill {
                    name: format!("{SKILL_DIR}{name}"),
                    level,
                    enabled,
                    unk_205,
                    devotion_level: 0,
                    devotion_experience: 0,
                    sublevel: 0,
                    skill_active: 0,
                    skill_transition: 0,
                    autocast_skill: String::new(),
                    autocast_controller: String::new(),
                })
                .collect(),
            masteries_allowed: 0,
            skill_reclamation_points_used: 0,
            devotion_reclamation_points_used: 0,
            item_skills: Vec::new(),
            extra_skills: Vec::new(),
        },
        notes: Notes {
            version: 1,
            notes: Vec::new(),
        },
        factions: Factions {
            version: 5,
            current: 0,
            factions: NEW_FACTION_VALUES
                .iter()
                .map(|&value| Faction {
                    modified: 0,
                    unlocked: 0,
                    value,
                    positive_boost: 0.0,
                    negative_boost: 0.0,
                })
                .collect(),
        },
        ui: UiSettings {
            version: 7,
            unk1: 0,
            unk2: 0,
            unk3: 0,
            slots: (0..5)
                .map(|i| UiSlot {
                    a: String::new(),
                    b: String::new(),
                    flag: u8::from(i == 0),
                })
                .collect(),
            slots_per_bar: 47,
            bars: vec![HotSlotBar {
                id: 0,
                slots: hotbar,
            }],
            camera_distance: 36.0,
        },
        tutorials: Tutorials {
            version: 1,
            pages: vec![7, 69],
        },
        stats: PlayStats {
            version: 12,
            play_time: 0,
            deaths: 0,
            kills: 0,
            experience_from_kills: 0,
            health_potions_used: 0,
            mana_potions_used: 0,
            max_level: 1,
            hits_received: 0,
            hits_inflicted: 0,
            crits_inflicted: 0,
            crits_received: 0,
            greatest_damage_inflicted: 0.0,
            per_difficulty: vec![per_diff(), per_diff(), per_diff()],
            champion_kills: 0,
            last_hit: 0.0,
            last_hit_by: 0.0,
            greatest_damage_received: 0.0,
            hero_kills: 0,
            items_crafted: 0,
            relics_crafted: 0,
            transcendent_relics_crafted: 0,
            mythical_relics_crafted: 0,
            shrines_restored: 0,
            one_shot_chests_opened: 0,
            lore_notes_collected: 0,
            boss_kills: [0; 3],
            survival_greatest_wave: 0,
            survival_greatest_score: 0,
            survival_defenses_built: 0,
            survival_powerups_activated: 0,
            skill_stats: Vec::new(),
            endless_souls: 0,
            endless_essence: 0,
            difficulty_skip: 0,
            ultimate_veteran_boss_kills: 0,
            hidden_chests_opened: 0,
            player_u4901: 0,
            player_u4902: 0,
        },
        tokens: Tokens {
            version: 2,
            lists: Default::default(),
        },
    }
}

/// Skill-record prefix of `NEW_SKILLS`.
const SKILL_DIR: &str = "records/skills/";

/// `(record, level, enabled, unk_205)` of the skills a level-1 character starts with;
/// every other `Skill` field is 0 / empty.
const NEW_SKILLS: [(&str, u32, u8, u8); 54] = [
    ("default/defaultkickattack.dbr", 1, 1, 0),
    ("default/defaultwpbasicattack.dbr", 1, 1, 0),
    ("default/defaultmoveto.dbr", 1, 1, 0),
    ("default/defaultweaponattack.dbr", 1, 1, 0),
    ("default/defaultpetattack.dbr", 1, 0, 0),
    ("default/defaulthealthpotion.dbr", 1, 1, 0),
    ("default/defaultmanapotion.dbr", 1, 1, 0),
    ("default/defaultevade.dbr", 1, 1, 0),
    (
        "itemskillsgdx3/potionmodifiers/container_healthpotion_flask.dbr",
        0,
        0,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/container_healthpotion_tincture.dbr",
        0,
        0,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/container_healthpotion_vial.dbr",
        0,
        0,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_healovertime.dbr",
        1,
        1,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_absorbing.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_amplification.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_bloodrite.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_electrified.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_energyovertime.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_explosive.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_freezeimmunity.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_infusion.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_instantenergy.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_instantheal.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_pure.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_savior.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_selfless.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_stoneskin.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_swift.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_transfusion.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_truesight.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_vampire.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/healthpotion_wealth.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/container_energypotion_flask.dbr",
        0,
        0,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/container_energypotion_tincture.dbr",
        0,
        0,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/container_energypotion_vial.dbr",
        0,
        0,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_healovertime.dbr",
        0,
        0,
        0,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_absorbing.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_amplification.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_bloodrite.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_electrified.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_energyovertime.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_explosive.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_freezeimmunity.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_infusion.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_instantenergy.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_instantheal.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_pure.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_savior.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_selfless.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_stoneskin.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_swift.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_transfusion.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_truesight.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_vampire.dbr",
        0,
        0,
        1,
    ),
    (
        "itemskillsgdx3/potionmodifiers/energypotion_wealth.dbr",
        0,
        0,
        1,
    ),
];

/// `Faction::value` of the 47 factions of a fresh character; all other fields are 0.
pub(crate) const NEW_FACTION_VALUES: [f32; 47] = [
    24000.0, 0.0, -1.0, -1.0, -1.0, -1.0, 0.0, 0.0, 0.0, -1000.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0,
    0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, -1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

/// Loot-filter flags: the first 18 categories and #39 enabled.
const NEW_LOOT_FILTERS: [u8; 42] = [
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 1, 0, 0,
];
