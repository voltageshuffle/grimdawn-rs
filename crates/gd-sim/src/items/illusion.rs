//! Illusions (`transmutes`): an equipped item drawn with another item's look.
//!
//! - The item type of a record class ([`item_type`], `GAME::Item_Type`, Item
//!   vtable +1360; also the stash sort's rank), from the per-class
//!   constants of Game.dll.
//! - Which types take illusions ([`takes_illusion`]):
//!   `Item::IsTransmuteCompatible` (head 1, chest 3, legs 4, feet 5, hands 7,
//!   off-hands / shields 8, weapons 9, shoulders 14, medals 15).
//! - Which unlocked appearance fits a worn item ([`fits`]):
//!   GrimDawn.exe transmuter page builder `sub_1402817E0`: the list of the
//!   worn item's type; weapons by `GetWeaponType` ([`weapon_type`]): one-handed
//!   melee 2..=6 (axe, sword, mace, dagger, scepter) take each other, 15..=18
//!   (two-handed melee) take each other, 7..=14 (spear, guns, staff, shield,
//!   off-hand) only their own type.
//! - List order ([`sort_key`]): comparator `sub_140282080`: the three
//!   `records/items/transmutes/invisible_*` first, then by classification,
//!   item level, then the full display name (`GetGameDescription` without
//!   colour: `Armor::GetGameDescription` joins quality, style and base tags,
//!   so "Empowered Skytear Hat" precedes "Praetorian Faceguard";
//!   `naming::record_description`), all ascending, UTF-16 code unit order.
//! - Gender: page builder `sub_1402817E0` skips a look when
//!   `Item::IsMaleCompatible` is false: `preventMaleTransmute` set and the
//!   look's gender (the main player's, `AddItemToTransmutes` calls
//!   `SetGender(GetGender())`; 1 = male) is male.
//! - Cost ([`cost`]): `sub_140282C30`: the window record's
//!   `transmuteCost` equation (`playerLevel`, `itemLevel`), `floor(max(v, 1) +
//!   0.5)` per changed item; apply `sub_140282880`: a pick whose look matches
//!   the item's own ([`same_look`], `HasMatchingAppearance(id, true)`) clears
//!   the illusion for free, any other pick costs one `cost`.
//! - Unlocking ([`same_look`] dedupes):
//!   `GameEngine::AddItemToTransmutes`: an item added to the player's
//!   inventory (or loaded with it) whose type takes illusions is stored under
//!   its type unless a stored record has the same name or the same look
//!   (`ItemEquipment::HasMatchingAppearance`: mesh, base / bump / spec / glow
//!   textures and shader; PORT: compared as the records' mesh and texture
//!   fields, the engine compares the gendered actor's loaded ones). An
//!   `ItemTransmuter` (DLC unlock item) stores the record it unlocks
//!   (`transmuteDbr`).
//! - The look of an item ([`look_record`]): its `transmute_name` when set
//!   (`ItemEquipment::UpdateTransmute`), else its own record.
use gd_formats::arz::Record;
use gd_formats::save::item::Item;

/// `records/items/transmutes/invisible_{head,shoulders,medal}.dbr`: listed
/// first (`sub_140282080`).
pub const INVISIBLE: [&str; 3] = [
    "records/items/transmutes/invisible_head.dbr",
    "records/items/transmutes/invisible_shoulders.dbr",
    "records/items/transmutes/invisible_medal.dbr",
];

/// `GAME::Item_Type` of a record class (Item vtable +1360), 0 for none.
pub fn item_type(class: &str) -> u32 {
    match class {
        "ArmorProtective_Head" => 1,
        "ArmorJewelry_Amulet" => 2,
        "ArmorProtective_Chest" | "ArmorMisc_Clothing" | "ArmorMisc_Vestment" => 3,
        "ArmorProtective_Legs" => 4,
        "ArmorProtective_Feet" => 5,
        "ArmorJewelry_Ring" => 6,
        "ArmorProtective_Hands" | "ArmorJewelry_Bracelet" => 7,
        c if c.starts_with("WeaponArmor") => 8,
        c if c.starts_with("WeaponMelee")
            || c.starts_with("WeaponHunting")
            || c.starts_with("WeaponMagical") =>
        {
            9
        }
        "ItemArtifact" => 11,
        c if c.ends_with("Formula") => 12,
        "ArmorProtective_Waist" => 13,
        "ArmorProtective_Shoulders" => 14,
        "ArmorJewelry_Medal" => 15,
        "ItemRelic" | "ItemCharm" => 16,
        "OneShot_Sack" => 18,
        c if c.starts_with("OneShot") => 17,
        "ItemNote" => 20,
        "ItemEnchantment" => 21,
        "ItemTransmuter" | "ItemTransmuterSet" => 22,
        "ItemFactionWarrant" | "ItemFactionBooster" => 23,
        "ItemDevotionReset" => 24,
        "ItemDifficultyUnlock" => 25,
        "ItemAttributeReset" => 26,
        _ => 0,
    }
}

/// `Item::IsTransmuteCompatible`.
pub fn takes_illusion(item_type: u32) -> bool {
    matches!(item_type, 1 | 3 | 4 | 5 | 7 | 8 | 9 | 14 | 15)
}

/// `GetWeaponType` of a weapon class (0 for non-weapons), from the
/// per-class constants (Game.dll vtable +1712).
pub fn weapon_type(class: &str) -> u32 {
    match class {
        "WeaponMelee_Axe" => 2,
        "WeaponMelee_Sword" => 3,
        "WeaponMelee_Mace" => 4,
        "WeaponMelee_Dagger" => 5,
        "WeaponMelee_Scepter" => 6,
        "WeaponHunting_Spear" => 7,
        "WeaponHunting_Ranged2h" => 8,
        "WeaponMagical_Staff" => 9,
        "WeaponArmor_Shield" => 11,
        "WeaponArmor_Offhand" => 12,
        "WeaponHunting_Ranged1h" => 13,
        "WeaponMelee_Axe2h" => 15,
        "WeaponMelee_Sword2h" => 16,
        "WeaponMelee_Mace2h" => 17,
        "WeaponMelee_Spear2h" => 18,
        _ => 0,
    }
}

/// Whether an unlocked appearance of class `candidate` is offered for a worn
/// item of class `worn` (same item type, the weapon groups).
pub fn fits(worn: &str, candidate: &str) -> bool {
    if item_type(worn) != item_type(candidate) || !takes_illusion(item_type(worn)) {
        return false;
    }
    let (w, c) = (weapon_type(worn), weapon_type(candidate));
    if w == 0 || c == 0 {
        return w == c;
    }
    match w {
        2..=6 => (2..=6).contains(&c),
        7..=14 => c == w,
        15..=18 => (15..=18).contains(&c),
        _ => false,
    }
}

/// The record an item is drawn with.
pub fn look_record(item: &Item) -> &str {
    if item.transmute_name.is_empty() {
        &item.base_name
    } else {
        &item.transmute_name
    }
}

const LOOK_FIELDS: [&str; 16] = [
    "mesh",
    "armorMaleMesh",
    "armorFemaleMesh",
    "baseTexture",
    "bumpTexture",
    "specTexture",
    "glowTexture",
    "armorMaleBaseTexture",
    "armorMaleBumpTexture",
    "armorMaleSpecTexture",
    "armorMaleGlowTexture",
    "armorFemaleBaseTexture",
    "armorFemaleBumpTexture",
    "armorFemaleSpecTexture",
    "armorFemaleGlowTexture",
    "shader",
];

/// `HasMatchingAppearance` of two records (see the module doc).
pub fn same_look(a_name: &str, a: &Record, b_name: &str, b: &Record) -> bool {
    if a_name.eq_ignore_ascii_case(b_name) {
        return true;
    }
    if item_type(&a.class) != item_type(&b.class) {
        return false;
    }
    let mesh = |r: &Record| {
        r.str("mesh")
            .or(r.str("armorMaleMesh"))
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    if mesh(a).is_empty() || mesh(a) != mesh(b) {
        return false;
    }
    LOOK_FIELDS.iter().all(|f| {
        a.str(f)
            .unwrap_or("")
            .eq_ignore_ascii_case(b.str(f).unwrap_or(""))
    })
}

/// The list order key of an unlocked record (`name` = its display name,
/// keyed as UTF-16 code units like the engine's wide strings).
pub fn sort_key(record: &str, rec: &Record, name: String) -> (bool, u32, i32, Vec<u16>) {
    let invisible = INVISIBLE.iter().any(|i| i.eq_ignore_ascii_case(record));
    let class = super::Classification::parse(rec.str("itemClassification").unwrap_or("")) as u32;
    (
        !invisible,
        class,
        rec.i32("itemLevel").unwrap_or(0),
        name.encode_utf16().collect(),
    )
}

/// The cost of one changed item: the `transmuteCost` equation at the
/// player's level.
pub fn cost(equation: &str, player_level: u32, item_level: u32) -> u64 {
    let v = crate::expr::Expr::parse(equation).ok().map(|e| {
        e.eval(&|n: &str| match n {
            "playerLevel" => Some(player_level as f32),
            "itemLevel" => Some(item_level as f32),
            _ => None,
        })
    });
    (v.unwrap_or(0.0).max(1.0) + 0.5).floor() as u64
}
