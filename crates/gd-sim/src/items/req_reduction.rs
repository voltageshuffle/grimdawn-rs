//! Requirement reductions (`…ReqReduction` character attributes, ids 29-54).
//!
//! `ItemEquipment::AreRequirementsMet` /
//! `GetUIRequirementText` / `Item::AreRequirementsMet`
//! / `Item::GenerateRequirementText`: the
//! character's accumulator value (`GetValue`) of the item's attribute ids is
//! summed per requirement: Physique / Cunning / Spirit take the class
//! family's id (`ItemEquipment.reqReductionIds`, set by the class: melee 32-34, hunting
//! 35-37, staff 38-40, shield / off-hand 44-46, protective armour 47-49,
//! jewellery 50-52), a weapon adds the Weapon ids (29-31), a two-handed melee
//! weapon the 2H ids (41-43), and the global one (53) when above 0 adds to
//! all three. The level takes id 54 alone. The reduced need is
//! `trunc(need - pct * 0.01 * need + 0.5)`; a need of 0 is always met.
use crate::attributes::CharAttr;
use crate::stats::CharAttrAccumulator;

/// First id of the family blocks, in id order: Weapon, Melee, Hunting, Staff,
/// Weapon2H, Shield, Armor, Jewelry (three ids each: Physique, Cunning,
/// Spirit).
const FIRST: u8 = 29;
const GLOBAL: u8 = 53;
const LEVEL: u8 = 54;
const WEAPON: usize = 0;
const TWO_HANDED: usize = 4;

/// The character's reduction values by attribute id (29..=54).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reduction(pub [f32; 26]);

impl Reduction {
    pub fn from_acc(acc: &CharAttrAccumulator) -> Self {
        let mut r = [0.0; 26];
        for (i, v) in r.iter_mut().enumerate() {
            *v = CharAttr::from_id(FIRST + i as u8).map_or(0.0, |a| acc.get(a));
        }
        Reduction(r)
    }

    fn id(&self, id: u8) -> f32 {
        self.0[(id - FIRST) as usize]
    }

    /// The level requirement's reduction in percent (attribute 54).
    pub fn level(&self) -> f32 {
        self.id(LEVEL)
    }

    /// Physique / Cunning / Spirit reduction in percent for an item of `class`.
    pub fn for_class(&self, class: &str) -> [f32; 3] {
        // At most three family blocks (Weapon, the weapon kind, 2H).
        let mut families = [0usize; 3];
        let mut n = 0;
        let mut add = |f: usize| {
            families[n] = f;
            n += 1;
        };
        if class.starts_with("WeaponArmor") {
            add(5);
        } else if class.starts_with("Weapon") {
            add(WEAPON);
            add(match class.split('_').next() {
                Some("WeaponMelee") => 1,
                Some("WeaponHunting") => 2,
                _ => 3,
            });
            if super::resolve::requirement_prefix(class) == Some("melee2h") {
                add(TWO_HANDED);
            }
        } else if class.starts_with("ArmorJewelry") {
            add(7);
        } else if class.starts_with("Armor") {
            add(6);
        }
        let families = &families[..n];
        let global = self.id(GLOBAL).max(0.0);
        let sum = |k: usize| {
            families
                .iter()
                .map(|f| self.id(FIRST + (*f * 3 + k) as u8))
                .sum::<f32>()
                + global
        };
        [sum(0), sum(1), sum(2)]
    }
}

/// The need after a reduction of `pct` percent (`GenerateRequirementText`,
/// `AreRequirementsMet`: truncated after +0.5, never below 0).
pub fn reduced(need: f32, pct: f32) -> f32 {
    (((need - (pct * 0.01) * need) + 0.5) as i32).max(0) as f32
}

/// Whether `have` meets `need` with `pct` percent reduction (a need of 0 is met).
pub fn met(have: f32, need: f32, pct: f32) -> bool {
    need == 0.0 || have >= reduced(need, pct)
}
