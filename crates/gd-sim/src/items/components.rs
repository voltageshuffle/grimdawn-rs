//! Components (`ItemRelic`) socketed into equipment.
//!
//! (Game.dll): `ItemRelic::ItemUseFilter` maps the target item's class to one
//! of the component's flags (`ItemRelic::Load` field names); an item that
//! already has a component takes no other (`ItemRelic::CanRelicBeUsedOn`);
//! partial components (`var1` shards below `completedRelicLevel`) combine only
//! with the same record (`TestCompatibility`) and cannot be socketed.
use super::ItemDb;
use super::loot::RANDOMIZER_ENTRIES;
use gd_formats::arz::Record;
use gd_rules::Rng;

/// The component flag that allows a target item class.
pub fn class_flag(item_class: &str) -> Option<&'static str> {
    Some(match item_class {
        "ArmorJewelry_Amulet" => "amulet",
        "ArmorJewelry_Bracelet" => "bracelet",
        "ArmorJewelry_Ring" => "ring",
        "ArmorJewelry_Medal" => "medal",
        "ArmorProtective_Hands" => "hands",
        "ArmorProtective_Head" => "head",
        "ArmorProtective_Feet" => "feet",
        "ArmorProtective_Chest" => "chest",
        "ArmorProtective_Legs" => "legs",
        "ArmorProtective_Shoulders" => "shoulders",
        "ArmorProtective_Waist" => "waist",
        "WeaponArmor_Shield" => "shield",
        "WeaponArmor_Offhand" => "offhand",
        "WeaponHunting_Ranged1h" => "ranged1h",
        "WeaponHunting_Ranged2h" => "ranged2h",
        "WeaponHunting_Spear" => "spear",
        "WeaponMagical_Staff" => "staff",
        "WeaponMelee_Axe" => "axe",
        "WeaponMelee_Mace" => "mace",
        "WeaponMelee_Sword" => "sword",
        "WeaponMelee_Dagger" => "dagger",
        "WeaponMelee_Scepter" => "scepter",
        "WeaponMelee_Axe2h" => "axe2h",
        "WeaponMelee_Mace2h" => "mace2h",
        "WeaponMelee_Sword2h" => "sword2h",
        "WeaponMelee_Spear2h" => "spear2h",
        _ => return None,
    })
}

/// Shards a component needs (`completedRelicLevel`, at least 1).
pub fn shards_needed(component: &Record) -> u32 {
    component.i32("completedRelicLevel").unwrap_or(1).max(1) as u32
}

/// Whether a component item (`var1` shards) is complete (`ItemRelic::IsComplete`:
/// level ≥ `completedRelicLevel`; a `var1` of 0 counts as level 1).
pub fn is_complete(component: &Record, var1: u32) -> bool {
    var1.max(1) >= shards_needed(component)
}

pub fn fits(component: &Record, item_class: &str) -> bool {
    component.class == "ItemRelic" && class_flag(item_class).is_some_and(|f| component.bool(f))
}

/// The completion bonus of a relic (`ItemArtifact`); components get none
/// (`ItemRelic::Load` ignores `bonusTableName`). One weighted pick from
/// `bonusTableName`, level limits ignored (`ItemArtifact::InitializeItem`:
/// `r % (total + 1)`, first cumulative weight ≥ r, u32 wrapping); only the
/// first [`RANDOMIZER_ENTRIES`] entries count (`LootRandomizerTable::ManualLoad`).
pub fn roll_bonus(db: &ItemDb, component: &Record, rng: &mut Rng) -> String {
    if component.class != "ItemArtifact" {
        return String::new();
    }
    let Some(table) = component
        .str("bonusTableName")
        .filter(|t| !t.is_empty())
        .and_then(|t| db.get(t))
    else {
        return String::new();
    };
    let mut list = Vec::new();
    for i in 1..=RANDOMIZER_ENTRIES {
        let name = table.str(&format!("randomizerName{i}")).unwrap_or("");
        let w = table.i32(&format!("randomizerWeight{i}")).unwrap_or(0);
        if !name.is_empty() && w > 0 {
            list.push((name.to_string(), w as u32));
        }
    }
    let total = list.iter().fold(0u32, |t, x| t.wrapping_add(x.1));
    if list.is_empty() {
        return String::new();
    }
    let r = rng.next() % total.wrapping_add(1).max(1);
    let mut acc = 0u32;
    for (n, w) in list {
        acc = acc.wrapping_add(w);
        if r <= acc {
            return n;
        }
    }
    String::new()
}
