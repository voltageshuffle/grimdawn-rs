//! Potion containers and modifiers (Forgotten Gods-era potion system,
//! `records/skills/itemskillsgdx3/potionmodifiers/`).
//!
//! (Game.dll): every character has the potion skills; a skill's
//! `skillLocked` record flag is its initial lock (`Skill::LoadResources`,
//! saved per character as `SkillState`'s byte after `skillEnabled`, the
//! save's `unk_205`). `OneShot_SkillUnlock` items unlock them account-wide
//! (`GameEngine::AddPotionUnlock`, re-applied to every character on load):
//! for each of the item's `skillNames`, a `Skill_PotionContainer`'s unlock
//! level rises to the item's `skillLevel` and any locked skill unlocks.
//! The item can be used (`AllowUse`) while one of its containers is below
//! that level or one of its skills is still locked. A container keeps its
//! unlock level in `SkillActiveState` (`skillSubLevel`, the save's
//! `sublevel`) and its chosen tier as the skill level.
//!
//! The potions window ("Cauldron", `records/ui/cauldron/potions_table.dbr`, GrimDawn.exe):
//! - lists ([`trees`], loader `PotionsWindow::LoadTrees`): per potion (the player's
//!   health / energy `Skill_ChargePotion`, `customPotionTree1/2`), its
//!   `Skill_PotionContainer`s in tree order and its modifiers: the skill's
//!   modifiers in tree order, then its secondary skills (`SkillSecondary_*`)
//!   in tree order;
//! - the two modifier slots show the potion's enabled modifiers in that list
//!   order ([`slots`], open `PotionsWindow::Open`), not in choice order; they are
//!   disabled while no container of the potion is chosen
//!   ([`slots_enabled`], update `PotionsWindow::Update`);
//! - a click on a container that is not enabled chooses it
//!   ([`choose_container`], click `PotionsWindow::OnButtonPress` → `PotionsWindow::ApplyChoice`): every
//!   other container of the potion turns off (one per potion);
//! - a modifier picked for a slot ([`place_modifier`], `PotionsWindow::PlaceModifier` →
//!   `PotionsWindow::ApplyChoice`): nothing when the slot already holds it; the other slot
//!   is emptied when it holds the same modifier or both are
//!   `exclusivePotionSkill` (`SkillProfile::IsExclusivePotionSkill`), else
//!   kept; the picked and kept modifiers turn on (level 1 when at 0), every
//!   other unlocked modifier of the potion turns off, and so does the other
//!   potion's unlocked modifier at the same position in its unlocked list
//!   as the picked or the kept one.
//!
//! [`select`] (hook `GD_POTION_SELECT`, not a game path) takes no slot: a modifier goes into
//! the first empty slot, else the second.
//!
//! The potion's bitmap ([`potion_bitmap`] Game.dll
//! `Skill_ChargePotion::UpdatePotionBitmap`, run by its `Update` every frame,
//! and `GameEngine::GetPotionModifierComboBitmap`): the combos are the
//! `healthPotionModifierCombos` then `energyPotionModifierCombos` records
//! of `gameascendant.dbr` (`GameEngine::LoadFromDatabase`; `container`,
//! `modifier1` / `modifier2` lists, `potionTexture`, `potionMask`,
//! `maskTopOffset` / `maskBottomOffset`), grouped by container in list
//! order. The key is the potion's enabled container (the last one) and its
//! enabled modifiers plus its secondary skills with a level, sorted (byte
//! order). The first combo of that container wins where: with one modifier
//! chosen, a combo naming both lists is skipped; with none, the combo has
//! empty lists; else the *last* sorted modifier is in `modifier1` or
//! `modifier2` (the engine's loop keeps only the last name's result). No
//! match keeps the previous bitmap (empty at first). With no container
//! enabled the key is the empty name (`UpdatePotionBitmap` passes an empty
//! string, `GameEngine::GetPotionModifierComboBitmap` looks it up like any
//! other): `LoadFromDatabase` files a combo without a `container` under it,
//! the stock `potionhealth_default.dbr` (`modifier1` the heal over time) and
//! `potionenergy_default.dbr` (no lists): no texture (the slot keeps the
//! skill's icon) but the mask `cauldron_potion01a-01_msk.tex`, offsets 9 / 9. PORT: "enabled" is
//! level > 0 (`IsSkillEnabled` also checks the skill's enabled flags);
//! names compare case-insensitively (the engine compares exact record
//! names, stored lower case).
use crate::skills::SkillBook;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;

/// Containers and modifiers: `Skill_PotionModifier` and the secondary
/// effects (`SkillSecondary_*`), not their `_buff` records.
fn is_potion_skill(class: &str) -> bool {
    matches!(class, "Skill_PotionContainer" | "Skill_PotionModifier")
        || class.starts_with("SkillSecondary_")
}

/// Adds the potion skills a character lacks (level 0, locked per record): every skill of the
/// player's two potion trees (`customPotionTree1/2`) in tree order.
pub fn ensure_skills(db: &Database, book: &mut SkillBook) {
    let Some(pc) = db.get(PLAYER) else {
        return;
    };
    let names: Vec<String> = ["customPotionTree1", "customPotionTree2"]
        .iter()
        .filter_map(|k| pc.str(k))
        .flat_map(|tree| crate::skills::tree_skills(db, tree))
        .collect();
    for n in names {
        if book.skills.iter().any(|(s, _)| s.eq_ignore_ascii_case(&n)) {
            continue;
        }
        let Some(r) = db.get(&n) else { continue };
        if !is_potion_skill(&r.class) {
            continue;
        }
        // A missing `skillLocked` reads as locked.
        if r.i32("skillLocked").unwrap_or(1) != 0 {
            book.locked.insert(n.to_ascii_lowercase());
        }
        book.skills.push((n, 0));
    }
    book.touched();
}

/// `OneShot_SkillUnlock::AllowUse`.
pub fn can_unlock(db: &Database, book: &SkillBook, item: &Record) -> bool {
    let level = item.i32("skillLevel").unwrap_or(0).max(0) as u32;
    item.strs("skillNames").iter().any(|s| {
        let low = s.to_ascii_lowercase();
        match db.get(s) {
            Some(r) if r.class == "Skill_PotionContainer" => {
                book.unlock.get(&low).copied().unwrap_or(0) < level
            }
            Some(_) => book.locked.contains(&low),
            None => false,
        }
    })
}

/// `GameEngine::AddPotionUnlock` on the character.
pub fn apply_unlock(db: &Database, book: &mut SkillBook, item: &Record) {
    let level = item.i32("skillLevel").unwrap_or(0).max(0) as u32;
    for s in item.strs("skillNames") {
        let low = s.to_ascii_lowercase();
        let Some(r) = db.get(s) else { continue };
        if !book.skills.iter().any(|(n, _)| n.eq_ignore_ascii_case(s)) {
            book.skills.push((s.to_owned(), 0));
        }
        if r.class == "Skill_PotionContainer" {
            let u = book.unlock.entry(low.clone()).or_insert(0);
            *u = (*u).max(level);
        }
        book.locked.remove(&low);
    }
    book.touched();
}

/// The player record holding the potion skill trees.
pub const PLAYER: &str = "records/creatures/pc/malepc01.dbr";

/// One potion of the window: its skill, containers and modifiers in the window's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PotionTree {
    pub potion: String,
    pub containers: Vec<String>,
    pub modifiers: Vec<String>,
}

/// The health and energy potion trees (`customPotionTree1`, `2`).
pub fn trees(db: &Database) -> Vec<PotionTree> {
    let Some(pc) = db.get(PLAYER) else {
        return Vec::new();
    };
    ["customPotionTree1", "customPotionTree2"]
        .iter()
        .filter_map(|k| pc.str(k))
        .map(|tree| {
            let mut t = PotionTree::default();
            let mut secondary = Vec::new();
            for sk in crate::skills::tree_skills(db, tree) {
                let rec = db.get(&sk);
                let class = rec.as_ref().map_or("", |r| r.class.as_str());
                if class == "Skill_PotionContainer" {
                    t.containers.push(sk);
                } else if class.starts_with("SkillSecondary_") {
                    secondary.push(sk);
                } else if crate::skills::is_modifier_class(class) {
                    t.modifiers.push(sk);
                } else if t.potion.is_empty() {
                    t.potion = sk;
                }
            }
            t.modifiers.extend(secondary);
            t
        })
        .collect()
}

fn on(book: &SkillBook, skill: &str) -> bool {
    book.level(skill) > 0
}

fn is_locked(book: &SkillBook, skill: &str) -> bool {
    book.locked.contains(&skill.to_ascii_lowercase())
}

/// The modifiers in the potion's two slots.
pub fn slots(book: &SkillBook, tree: &PotionTree) -> [Option<String>; 2] {
    slot_refs(book, tree).map(|s| s.cloned())
}

/// [`slots`], borrowed from the tree.
fn slot_refs<'a>(book: &SkillBook, tree: &'a PotionTree) -> [Option<&'a String>; 2] {
    let mut on_list = tree.modifiers.iter().filter(|m| on(book, m));
    [on_list.next(), on_list.next()]
}

/// Whether the potion's modifier slots take choices (a container is chosen).
pub fn slots_enabled(book: &SkillBook, tree: &PotionTree) -> bool {
    tree.containers.iter().any(|c| on(book, c))
}

/// The modifiers a slot can take: the potion's unlocked ones, in list order.
pub fn unlocked_modifiers<'a>(book: &SkillBook, tree: &'a PotionTree) -> Vec<&'a String> {
    tree.modifiers
        .iter()
        .filter(|m| !is_locked(book, m))
        .collect()
}

fn tree_of<'a>(trees: &'a [PotionTree], skill: &str) -> Option<(usize, &'a PotionTree)> {
    trees.iter().enumerate().find(|(_, t)| {
        t.containers
            .iter()
            .chain(&t.modifiers)
            .any(|s| s.eq_ignore_ascii_case(skill))
    })
}

/// Chooses a container (at its unlock level); the potion's other containers
/// turn off.
pub fn choose_container(
    db: &Database,
    book: &mut SkillBook,
    skill: &str,
) -> Result<(), &'static str> {
    let trees = trees(db);
    let (_, tree) = tree_of(&trees, skill).ok_or("no potion")?;
    if !tree
        .containers
        .iter()
        .any(|c| c.eq_ignore_ascii_case(skill))
    {
        return Err("not a potion container");
    }
    let tier = book
        .unlock
        .get(&skill.to_ascii_lowercase())
        .copied()
        .unwrap_or(0);
    if is_locked(book, skill) || tier == 0 {
        return Err("not unlocked");
    }
    for c in &tree.containers {
        set_level_of(&mut book.skills, c, 0);
    }
    set_level(book, skill, tier);
    book.touched();
    Ok(())
}

/// Puts a modifier in slot `slot` (0 or 1) of its potion (`PotionsWindow::PlaceModifier`).
pub fn place_modifier(
    db: &Database,
    book: &mut SkillBook,
    skill: &str,
    slot: usize,
) -> Result<(), &'static str> {
    let trees = trees(db);
    let (pi, tree) = tree_of(&trees, skill).ok_or("no potion")?;
    if !tree.modifiers.iter().any(|m| m.eq_ignore_ascii_case(skill)) {
        return Err("not a potion modifier");
    }
    if is_locked(book, skill) {
        return Err("locked");
    }
    let cur = slot_refs(book, tree);
    let slot = slot.min(1);
    if cur[slot].is_some_and(|c| c.eq_ignore_ascii_case(skill)) {
        return Ok(());
    }
    let exclusive = |n: &str| {
        db.get(n)
            .is_some_and(|r| r.i32("exclusivePotionSkill").unwrap_or(0) != 0)
    };
    let keep = cur[1 - slot]
        .filter(|o| !(o.eq_ignore_ascii_case(skill) || (exclusive(skill) && exclusive(o))));
    // `PotionsWindow::ApplyChoice`, with this potion's and the other potion's unlocked lists.
    let mine = unlocked_modifiers(book, tree);
    let theirs = trees
        .get(1 - pi.min(1))
        .map(|t| unlocked_modifiers(book, t))
        .unwrap_or_default();
    let chosen =
        |n: &str| n.eq_ignore_ascii_case(skill) || keep.is_some_and(|k| k.eq_ignore_ascii_case(n));
    for (i, m) in mine.iter().enumerate() {
        if chosen(m) {
            if book.level(m) == 0 {
                set_level(book, m, 1);
            }
            if let Some(t) = theirs.get(i) {
                set_level_of(&mut book.skills, t, 0);
            }
        } else {
            set_level_of(&mut book.skills, m, 0);
        }
    }
    book.touched();
    Ok(())
}

/// Chooses a container (at its unlock level) or modifier for its potion
/// (hook `GD_POTION_SELECT`).
pub fn select(db: &Database, book: &mut SkillBook, skill: &str) -> Result<(), &'static str> {
    let r = db.get(skill).ok_or("no such skill")?;
    if !is_potion_skill(&r.class) {
        return Err("not a potion container or modifier");
    }
    if r.class == "Skill_PotionContainer" {
        return choose_container(db, book, skill);
    }
    let trees = trees(db);
    let (_, tree) = tree_of(&trees, skill).ok_or("no potion")?;
    let slot = slot_refs(book, tree)
        .iter()
        .position(Option::is_none)
        .unwrap_or(1);
    place_modifier(db, book, skill, slot)
}

fn set_level_of(skills: &mut [(String, u32)], skill: &str, level: u32) {
    if let Some((_, l)) = skills
        .iter_mut()
        .find(|(n, _)| n.eq_ignore_ascii_case(skill))
    {
        *l = level;
    }
}

fn set_level(book: &mut SkillBook, skill: &str, level: u32) {
    match book
        .skills
        .iter_mut()
        .find(|(n, _)| n.eq_ignore_ascii_case(skill))
    {
        Some((_, l)) => *l = level,
        None => book.skills.push((skill.to_owned(), level)),
    }
}

/// A potion's combo bitmap (`Skill_ChargePotion::GetPotionBitmap` /
/// `GetPotionMask` / `GetMaskTopOffset` / `GetMaskBottomOffset`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PotionBitmap {
    pub texture: String,
    pub mask: String,
    pub mask_top: i32,
    pub mask_bottom: i32,
}

/// The bitmap of the potion's current container and modifiers; `None`: no combo matches
/// (the engine keeps the last one).
pub fn potion_bitmap(db: &Database, book: &SkillBook, tree: &PotionTree) -> Option<PotionBitmap> {
    let container = tree
        .containers
        .iter()
        .rfind(|c| on(book, c))
        .map_or("", String::as_str);
    // The sorted key list matters only by its length and its last (byte-order largest) name.
    let mut chosen = 0;
    let mut last: Option<String> = None;
    for m in tree.modifiers.iter().filter(|m| on(book, m)) {
        chosen += 1;
        let m = m.to_ascii_lowercase();
        if last.as_ref().is_none_or(|l| m > *l) {
            last = Some(m);
        }
    }
    let engine = db.get(crate::ascendant::GAME_ASCENDANT)?;
    let has = |list: &[&str], name: &str| list.iter().any(|m| m.eq_ignore_ascii_case(name));
    for path in engine
        .strs_iter("healthPotionModifierCombos")
        .chain(engine.strs_iter("energyPotionModifierCombos"))
    {
        let Some(c) = db.get(path) else { continue };
        if !c
            .str("container")
            .unwrap_or("")
            .eq_ignore_ascii_case(container)
        {
            continue;
        }
        let (m1, m2) = (c.strs("modifier1"), c.strs("modifier2"));
        let hit = match &last {
            _ if chosen == 1 && !m1.is_empty() && !m2.is_empty() => false,
            None => m1.is_empty() && m2.is_empty(),
            Some(last) => has(&m1, last) || has(&m2, last),
        };
        if hit {
            return Some(PotionBitmap {
                texture: c.str("potionTexture").unwrap_or("").to_owned(),
                mask: c.str("potionMask").unwrap_or("").to_owned(),
                mask_top: c.i32("maskTopOffset").unwrap_or(0),
                mask_bottom: c.i32("maskBottomOffset").unwrap_or(0),
            });
        }
    }
    None
}
