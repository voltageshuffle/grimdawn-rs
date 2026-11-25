//! The two weapon sets (weapon set I / II) and the weapon swap.
//!
//! State ([`gd_game_core::inventory::Inventory`]): `equipped[12..14]` hold the active
//! set's main / off hand, `other_weapons` the inactive set's, `alternate`
//! which set is active, `requested_alternate` the asked-for one and
//! `alternate_enabled` whether set II was ever chosen. Only the active set's
//! items are worn (the loadout, the profile, item skills, the body).
//!
//! Game.dll / Grim Dawn.exe:
//! - `EquipmentCtrl::SetAlternateEquipment`: the equipment's
//!   alternate flag (`EquipmentCtrl.alternate`) and the two weapon-set controllers (`EquipmentCtrl.weaponSet1`
//!   set I, `EquipmentCtrl.weaponSet2` set II) whose own `alternate` marks the inactive one; `WeaponSet__Sift`
//!   detaches an inactive set's item (and one whose requirements fail),
//!   attaches the active one's, then `AttributesHaveChanged` (the profile).
//!   The save's `WeaponSet::flag` is that inactive flag, `attached` the
//!   attach state (`gdc.rs`).
//! - `Player::SetAlternateConfig` only stores the request
//!   (`ControllerCharacter.alternateConfigRequest`); `Player::GetAlternateConfig` reads the
//!   current set (`Player.alternateConfig`). `ControllerCharacter::Update` applies a
//!   differing request when the character's action state is neither Attack
//!   (7) nor 20 (no name in `GetActionStateAsText`), no channel runs
//!   (`SkillManager::IsRunningSkill`: `SkillChanneled`, beams, drains,
//!   spins) and no active skill is in its warm-up (`Skill::GetWarmUpWasActive`:
//!   weapon charges, path charges, blinks, leaps,
//!   teleports, Skill_AttackRadiusSpin). Otherwise the request waits; there
//!   is no cooldown and no swap animation. Evading, moving or being in
//!   combat do not block it.
//! - The HUD button (`hudButtonWeaponSwap`, handler `UIPlayerHud::OnToolbarButton`): with a
//!   living player, `SetAlternateConfig(!GetAlternateConfig())` and
//!   `Character::ClearComboCharges`. The button is disabled (`UIPlayerHud::Update`:
//!   `UIPlayerHud.weaponSwapDisabled`) until `Player.alternateConfigEnabled` (`alternate_config_enabled`)
//!   is set, and its rollover is `hudButtonWeaponSwapRolloverSpecial`
//!   ("…you must equip a second set…") until then.
//! - The character window's swap button (`equipSwap1LeftButton` at its
//!   record position, the only one of the four `equipSwap*` fields the loader
//!   `CharacterWindow::Load` reads; rollover `equipSwapButtonRollover`): its toggle
//!   state follows `GetAlternateConfig` (`CharacterWindow::Update`); a click
//!   (`CharacterWindow::OnButtonEvent`) requests the toggled set, clears combo charges and, when
//!   that is set II, sets `Player.alternateConfigEnabled`. The Illusionist's copy
//!   (`TransmuteWindow::OnButtonEvent`,
//!   log "EquipSwap %u") does the same.
//! - Hotbar: the mouse buttons have a slot per set
//!   (`PlayerHotSlotCtrl::GetPrimarySlot` / `GetSecondarySlot`: slots 10 / 12
//!   for set I, 11 / 13 for set II by `GetAlternateConfig`).
//! - Item skills of a detached item leave the skill list
//!   (`SkillManager::RemoveItemSkills`, cached by item and set,
//!   `StoreCachedItemSkill`); their hotbar entries stay.
//! - The detach is the unequip's (`WeaponSet__Sift` → `SendEquipDetachAction`
//!   → `Weapon::DetachItem` → `ItemEquipment::DetachItem`): skill / mastery
//!   bonuses of the set left behind release the pets they reach
//!   (`crate::pets` module docs, release on a lost skill bonus).
//!
//! PORT: the displayed page's `Caster::slots[10]` / `[12]` always hold the
//! active set's mouse skills (every reader of `SLOT_LEFT` / `SLOT_RIGHT`
//! keeps working): a swap exchanges 10 ↔ 11 and 12 ↔ 13 on every page and
//! [`save_order`] puts them back in set order for the save. The swap clears
//! the combo charges (`crate::combo`). The W key presses the
//! HUD button (its key action 34); the button's own disabled state gates it.
use bevy::prelude::*;
use gd_game_core::casting::{Caster, SLOT_LEFT, SLOT_RIGHT, SLOTS};
use gd_game_core::casting::{Channel, Leap, PathCharge};
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::Dead;
use gd_game_core::inventory::{Inventory, InventorySet, Requirements, detach_rule};
use gd_game_core::player::Player;

pub struct WeaponSetsPlugin;

impl Plugin for WeaponSetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SwapWeapons>().add_systems(
            Update,
            (
                swap_requests.run_if(resource_exists::<gd_game_core::inventory::Inventory>),
                apply_swap.run_if(resource_exists::<gd_game_core::inventory::Inventory>),
            )
                .chain()
                .in_set(InventorySet)
                .after(gd_game_core::inventory::InventoryStage::Init)
                .before(gd_game_core::inventory::InventoryStage::Equipment),
        );
    }
}

/// Swap to the other weapon set (the HUD button / W, or the character
/// window's swap button with `from_window`).
#[derive(Message, Clone, Copy, Debug)]
pub struct SwapWeapons {
    pub from_window: bool,
}

/// Whether a swap request is taken: the HUD button is disabled until set II
/// was chosen once, and works only for a living player (`alive`); the
/// character window's always is. The one rule for the set change
/// ([`swap_requests`]) and the combo clear (`crate::combo`).
pub fn swap_accepted(r: &SwapWeapons, inv: &Inventory, alive: bool) -> bool {
    r.from_window || (inv.alternate_enabled && alive)
}

/// Turns swap requests into the requested set (see the module doc).
fn swap_requests(
    mut requests: MessageReader<SwapWeapons>,
    mut inv: ResMut<Inventory>,
    players: Query<(), (With<Player>, Without<Dead>)>,
) {
    for r in requests.read() {
        if !swap_accepted(r, &inv, !players.is_empty()) {
            continue;
        }
        let want = !inv.alternate;
        inv.requested_alternate = want;
        if r.from_window && want {
            inv.alternate_enabled = true;
        }
        inv.changed();
    }
}

/// Applies a pending set change once the player is free
/// (`ControllerCharacter::Update`): the weapons trade places, the mouse
/// slots follow and `sync_equipment` rebuilds the profile.
fn apply_swap(
    mut inv: ResMut<Inventory>,
    mut players: Query<
        (
            &Character,
            &mut Caster,
            &Requirements,
            Has<Channel>,
            Has<PathCharge>,
            Has<Leap>,
        ),
        With<Player>,
    >,
) {
    if inv.requested_alternate == inv.alternate {
        return;
    }
    let Ok((ch, mut caster, req, channel, path_charge, leap)) = players.single_mut() else {
        return;
    };
    // Attack (the port's Attack and Cast), a warm-up (the Charge approach,
    // path charges, leaps) or a channel holds the request.
    if matches!(
        ch.state,
        CharState::Attack | CharState::Cast | CharState::Charge
    ) || channel
        || path_charge
        || leap
    {
        return;
    }
    let inv = &mut *inv;
    for k in 0..2 {
        std::mem::swap(&mut inv.equipped[12 + k], &mut inv.other_weapons[k]);
    }
    // The swapped-in weapons attach or detach at once, by the sift's rule (`WeaponSet__Sift`), not at the next
    // sift: a weapon failing its requirements is never worn in between.
    for i in 12..14 {
        let detached = inv.equipped[i]
            .as_ref()
            .filter(|item| detach_rule(inv, i, item, req).0)
            .cloned();
        inv.detached[i] = detached;
    }
    inv.alternate = inv.requested_alternate;
    inv.mark_equipment_dirty();
    inv.changed();
    swap_mouse_slots(&mut caster);
    info!("weapon set {}", if inv.alternate { "II" } else { "I" });
}

/// Exchanges the two sets' mouse slots on every hotbar page.
pub fn swap_mouse_slots(caster: &mut Caster) {
    let swap = |p: &mut [Option<String>; SLOTS]| {
        p.swap(SLOT_LEFT, SLOT_LEFT + 1);
        p.swap(SLOT_RIGHT, SLOT_RIGHT + 1);
    };
    swap(&mut caster.slots);
    for p in caster.sets.values_mut() {
        swap(p);
    }
}

/// A hotbar page in save order (slots 10 / 12 = set I) when `alternate` is
/// the active set (see the module doc).
pub fn save_order(page: &[Option<String>; SLOTS], alternate: bool) -> [Option<String>; SLOTS] {
    let mut p = page.clone();
    if alternate {
        p.swap(SLOT_LEFT, SLOT_LEFT + 1);
        p.swap(SLOT_RIGHT, SLOT_RIGHT + 1);
    }
    p
}
