//! Components (`ItemRelic`) socketed into equipment.
//!
//! (Game.dll): `ItemRelic::ItemUseFilter` @0x18034a070 maps the
//! target item's class to one of the component's flags (`ItemRelic::Load`
//! @0x180347670 field names); an item that already has a component takes no
//! other (`ItemRelic::CanRelicBeUsedOn` @0x180348700, rules and the apply
//! mode in `gd_game::inventory`); partial components (`var1` shards
//! below `completedRelicLevel`) combine only with the same record
//! (`TestCompatibility` @0x18034a000) and cannot be socketed.
use super::ItemDb;
use super::loot::RANDOMIZER_ENTRIES;
use crate::combat::Rng;
use gd_formats::arz::Record;

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

/// Whether a component item (`var1` shards) is complete. The
/// constructor sets relic level 1 (@0x180347040), `ItemRelic::InitializeItem`
/// @0x180348260 replaces it with `var1` only when non-zero, and
/// `ItemRelic::IsComplete` @0x180348020 is level ≥ `completedRelicLevel`
/// (1 on every stock component, so 0 reads as complete there).
pub fn is_complete(component: &Record, var1: u32) -> bool {
    var1.max(1) >= shards_needed(component)
}

/// Whether `component` may go into an item of `item_class`.
pub fn fits(component: &Record, item_class: &str) -> bool {
    component.class == "ItemRelic" && class_flag(item_class).is_some_and(|f| component.bool(f))
}

/// The completion bonus a socketed component brings: none. In
/// Game.dll only `ItemArtifact::Load` reads `bonusTableName` (the component
/// records' entries are leftovers); `ItemRelic::Load` @0x180347670 does not,
/// and `ItemEquipment::AddRelic(ItemRelic*)` @0x1803426a0 copies the
/// component's own replica bonus field, which only `ItemArtifact::
/// InitializeItem` fills. For a relic (`ItemArtifact`) the bonus is one
/// weighted pick from its `bonusTableName` (a `LootRandomizerTable`), level
/// limits ignored (`ItemArtifact::InitializeItem` @0x18032a0c0:
/// `r % (total + 1)`, first cumulative weight ≥ r; the total and the running
/// sum are unsigned 32-bit and wrap, and a total of `u32::MAX` divides by 1).
/// The table loads through `LootRandomizerTable::ManualLoad`, so only its
/// first [`RANDOMIZER_ENTRIES`] entries count.
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
