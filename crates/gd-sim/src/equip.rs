//! The weapon loadout's hand state: `GAME::Weapon_Type` from the item class and
//! `EquipManager_HandState` (`EquipManager::GetHandState`), which picks the
//! animation set's record-key prefix ([`crate::anim::AnimSet::load`]) and qualifies weapon
//! skills ([`crate::weapon_pool`]).

/// `GAME::Weapon_Type` from the item class (`GetWeaponType` overrides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponType {
    Axe = 2,
    Sword = 3,
    Mace = 4,
    Dagger = 5,
    Scepter = 6,
    Spear = 7,
    Ranged2h = 8,
    Staff = 9,
    Magical = 10,
    Shield = 11,
    Offhand = 12,
    Ranged1h = 13,
    Axe2h = 15,
    Sword2h = 16,
    Mace2h = 17,
    Spear2h = 18,
}

impl WeaponType {
    pub fn from_class(class: &str) -> Option<Self> {
        Some(match class {
            "WeaponMelee_Axe" => Self::Axe,
            "WeaponMelee_Sword" => Self::Sword,
            "WeaponMelee_Mace" => Self::Mace,
            "WeaponMelee_Dagger" => Self::Dagger,
            "WeaponMelee_Scepter" => Self::Scepter,
            "WeaponHunting_Spear" => Self::Spear,
            "WeaponHunting_Ranged2h" => Self::Ranged2h,
            "WeaponMagical_Staff" => Self::Staff,
            "WeaponArmor_Shield" => Self::Shield,
            "WeaponArmor_Offhand" => Self::Offhand,
            "WeaponHunting_Ranged1h" => Self::Ranged1h,
            "WeaponMelee_Axe2h" => Self::Axe2h,
            "WeaponMelee_Sword2h" => Self::Sword2h,
            "WeaponMelee_Mace2h" => Self::Mace2h,
            "WeaponMelee_Spear2h" => Self::Spear2h,
            _ => return None,
        })
    }
    /// The weapons two hands can hold at once (dual wield).
    pub fn one_handed_melee(self) -> bool {
        matches!(
            self,
            Self::Axe | Self::Sword | Self::Mace | Self::Dagger | Self::Scepter
        )
    }
}

/// `EquipManager_HandState` (`EquipManager::GetHandState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandState {
    Unarmed = 0,
    OneHanded = 1,
    Axe2h = 2,
    Mace2h = 3,
    Sword2h = 4,
    Spear2h = 5,
    Spear = 6,
    Staff = 7,
    Ranged1h = 8,
    Ranged2h = 9,
    DualMelee = 10,
    DualRanged = 11,
    OneHandedShield = 12,
    Ranged1hShield = 13,
    SpearShield = 14,
    OneHandedOffhand = 15,
    Ranged1hOffhand = 16,
}

impl HandState {
    /// An appended variant must become the anchor here.
    pub const COUNT: usize = HandState::Ranged1hOffhand as usize + 1;

    /// The left hand is checked first (1h melee → dual wield, staff, 1h ranged → dual ranged), then
    /// the right weapon combined with a shield/off-hand in the left.
    pub fn from_weapons(right: Option<WeaponType>, left: Option<WeaponType>) -> Self {
        use WeaponType as W;
        if let Some(l) = left {
            if l.one_handed_melee() {
                return Self::DualMelee;
            }
            if l == W::Staff {
                return Self::Staff;
            }
            if l == W::Ranged1h {
                return Self::DualRanged;
            }
        }
        let Some(r) = right else { return Self::Unarmed };
        match r {
            r if r.one_handed_melee() => match left {
                Some(W::Shield) => Self::OneHandedShield,
                Some(W::Offhand) => Self::OneHandedOffhand,
                _ => Self::OneHanded,
            },
            W::Spear => {
                if left == Some(W::Shield) {
                    Self::SpearShield
                } else {
                    Self::Spear
                }
            }
            W::Ranged2h => Self::Ranged2h,
            W::Staff => Self::Staff,
            W::Ranged1h => match left {
                Some(W::Shield) => Self::Ranged1hShield,
                Some(W::Offhand) => Self::Ranged1hOffhand,
                _ => Self::Ranged1h,
            },
            W::Axe2h => Self::Axe2h,
            W::Sword2h => Self::Sword2h,
            W::Mace2h => Self::Mace2h,
            W::Spear2h => Self::Spear2h,
            _ => Self::Unarmed,
        }
    }

    /// Record-key prefix of the hand state's animation set (`Character::GetHandStateAnimation`).
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Unarmed => "unarmed",
            Self::OneHanded | Self::OneHandedShield | Self::OneHandedOffhand => "sHanded",
            Self::DualMelee => "dHanded",
            Self::Spear | Self::SpearShield => "spear",
            Self::Ranged2h => "ranged2h",
            Self::Staff => "staff",
            Self::DualRanged => "dualRanged",
            Self::Ranged1h | Self::Ranged1hShield | Self::Ranged1hOffhand => "ranged1h",
            Self::Axe2h => "axe2h",
            Self::Mace2h => "mace2h",
            Self::Sword2h => "sword2h",
            Self::Spear2h => "spear2h",
        }
    }

    /// Menu idle variant: the shield/off-hand sets read `<p>Shield…` / `<p>Offhand…` menu keys.
    pub(crate) fn menu_infix(self) -> &'static str {
        match self {
            Self::OneHandedShield | Self::Ranged1hShield => "Shield",
            Self::OneHandedOffhand | Self::Ranged1hOffhand => "Offhand",
            _ => "",
        }
    }
}
