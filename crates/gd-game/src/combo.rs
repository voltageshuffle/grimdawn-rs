//! The player's combo charges (rules and engine addresses: `gd_sim::combo`).
//!
//! [`Combo`] holds the charges (a component on the player, read-only for
//! the UI); [`ComboUse`] the skill use that may spend them, set when a cast
//! starts (`casting::try_cast`, the engine's reset on every skill request).
//! `combat::apply_hits` scales a use's hits by the multiplier taken on its
//! first hit, spends after that hit and gains charges from every skill hit
//! ([`on_hit`]). Charges expire here ([`tick`]) and clear on a weapon change,
//! the swap buttons and riftgate / portal travel ([`clear`]).
//!
//! PORT: the gain runs per hit message, not once per skill target result
//! (the lockout makes the two the same for a multi-target hit); a use spends
//! on its first hit only; the melee / ranged choice of the lockout is the
//! port's melee test (weapon skill without a ranged weapon) for the engine's
//! `AttackInfo` +148 flag; "player teleport" (sub_1805820F0) is riftgate /
//! portal travel. The look: the count's aura (`skillComboChargeAura`) as a
//! buff effect on the player; the "remain" effect / sound of a kept spend
//! is not played.
use crate::data::GameData;
use crate::inventory::Inventory;
use crate::player::Player;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_sim::combat::Rng;
use gd_sim::combo::{ComboCharges, ComboTable};

pub struct ComboPlugin;

impl Plugin for ComboPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                init,
                tick,
                clear,
                look.run_if(resource_exists::<ComboRules>),
            )
                .chain()
                .before(crate::combat::CombatSet)
                .after(crate::inventory::InventorySet),
        );
    }
}

/// The combo charges of the player.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct Combo(pub ComboCharges);

/// `gameengine.dbr`'s combo fields.
#[derive(Resource, Debug, Clone, Default)]
pub struct ComboRules(pub ComboTable);

/// The current skill use's claim on the charges (`Skill` +1460 / +1464).
#[derive(Component, Debug, Clone, Default)]
pub struct ComboUse {
    /// The used skill's data record (what its hits name).
    pub skill: String,
    /// The skill has a cooldown and is not channelled.
    pub eligible: bool,
    /// The multiplier taken by the use's first hit.
    pub multiplier: Option<f32>,
}

impl ComboUse {
    /// A use of `rec` (the cast's data record) with cooldown `cooldown` s.
    pub fn new(skill: &str, rec: &Record, cooldown: f32) -> Self {
        Self {
            skill: skill.to_owned(),
            eligible: cooldown > 0.0 && gd_sim::combo::class_takes_multiplier(&rec.class),
            multiplier: None,
        }
    }
}

fn init(
    mut commands: Commands,
    // Option: this system creates `ComboRules` once `GameData` exists.
    data: Option<Res<GameData>>,
    rules: Option<Res<ComboRules>>,
    players: Query<Entity, (With<Player>, Without<Combo>)>,
) {
    if rules.is_none()
        && let Some(data) = &data
    {
        let t = data
            .engine()
            .record
            .as_ref()
            .map(ComboTable::from_engine)
            .unwrap_or_default();
        commands.insert_resource(ComboRules(t));
    }
    for e in &players {
        commands.entity(e).insert(Combo::default());
    }
}

/// `SkillManager::Update`: the charges expire, the lockout runs out.
fn tick(time: Res<Time>, mut q: Query<&mut Combo>, mut carry: Local<f32>) {
    let ms = crate::session::frame_ms(&mut carry, &time);
    if ms <= 0 {
        return;
    }
    for mut c in &mut q {
        if c.0.remaining_ms > 0 || c.0.lockout_ms > 0 {
            c.0.update(ms);
        }
    }
}

/// `ClearComboCharges`: a weapon attached (the worn weapons changed), the
/// swap buttons, travel.
fn clear(
    mut fresh: crate::session::NewSession,
    // Option: the travel messages clear the combo without an inventory (no inventory takes no swap).
    inv: Option<Res<Inventory>>,
    mut swaps: MessageReader<crate::weapon_sets::SwapWeapons>,
    mut travel: MessageReader<crate::riftgates::GateTravel>,
    mut q: Query<&mut Combo, (With<Player>, Without<crate::combat::Dead>)>,
    mut worn: Local<Option<[Option<gd_formats::save::item::Item>; 2]>>,
) {
    if fresh.begun() {
        *worn = Default::default();
    }
    // Only a swap the weapon sets take clears (`weapon_sets::swap_accepted`).
    let alive = !q.is_empty();
    let mut now = swaps.read().fold(false, |any, r| {
        any | inv
            .as_ref()
            .is_some_and(|i| crate::weapon_sets::swap_accepted(r, i, alive))
    });
    now |= travel.read().count() > 0;
    if let Some(inv) = inv.filter(|i| i.is_changed()) {
        let weapons = [&inv.equipped[12], &inv.equipped[13]];
        if worn
            .as_ref()
            .is_some_and(|w| w[0] != *weapons[0] || w[1] != *weapons[1])
        {
            now = true;
        }
        *worn = Some(weapons.map(Clone::clone));
    }
    if now {
        for mut c in &mut q {
            if c.0.count > 0 {
                c.0.clear();
            }
        }
    }
}

const FX_KEY: &str = "combo charge";

/// The combo aura currently shown on the player.
#[derive(Component, Debug, Clone, Default)]
pub struct ComboAura(pub Option<String>);

/// `SetComboChargeEffect`: the count's aura on the player.
fn look(
    mut commands: Commands,
    data: Res<GameData>,
    rules: Res<ComboRules>,
    q: Query<(Entity, &Combo, Option<&ComboAura>), Changed<Combo>>,
    fx: Query<(Entity, &crate::visuals::BuffFx)>,
) {
    for (e, c, shown) in &q {
        let want = rules.0.aura(c.0.count);
        if shown.and_then(|s| s.0.as_deref()) == want {
            continue;
        }
        match want {
            Some(a) => {
                crate::casting::attach_buff_fx(&mut commands, &data, a, e, FX_KEY, false);
            }
            None => {
                for (f, b) in &fx {
                    if b.owner == e && b.skill == FX_KEY {
                        commands.entity(f).try_despawn();
                    }
                }
            }
        }
        commands
            .entity(e)
            .insert(ComboAura(want.map(str::to_owned)));
    }
}

/// What one hit needs from [`before_hit`] to [`on_hit`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HitCombo {
    /// (100 + multiplier) % for the hit's damage, 0 = none.
    pub scale: f32,
    /// The hit took the multiplier: it spends.
    pub spends: bool,
}

/// `CollectComboMultiplier` for a hit of `skill` (its data record name).
pub fn before_hit(
    table: &ComboTable,
    combo: &Combo,
    usage: Option<&mut ComboUse>,
    skill: Option<&str>,
) -> HitCombo {
    let (Some(u), Some(skill)) = (usage, skill) else {
        return HitCombo::default();
    };
    if !u.eligible || !u.skill.eq_ignore_ascii_case(skill) {
        return HitCombo::default();
    }
    let first = u.multiplier.is_none();
    let m = *u
        .multiplier
        .get_or_insert_with(|| table.multiplier(combo.0.count));
    if m == 0.0 {
        return HitCombo::default();
    }
    HitCombo {
        scale: m + 100.0,
        spends: first,
    }
}

/// After a hit: `SpendComboCharges` (`reduction`: the attacker's
/// `skillComboChargeSpendReduction`), then `IncrementComboCharge` from the
/// gaining skill's records (`gain`: the skill and its modifiers at their
/// levels, see [`gains_by_parent`]).
#[allow(clippy::too_many_arguments)]
pub fn on_hit(
    table: &ComboTable,
    combo: &mut Combo,
    hit: HitCombo,
    reduction: f32,
    rng: &mut Rng,
    gain: &[(&Record, usize)],
    melee: bool,
    attack_speed: f32,
) {
    if hit.spends {
        combo.0.spend(reduction, rng);
    }
    let (level, duration) = gd_sim::combo::skill_combo(gain);
    combo
        .0
        .increment(table, level, duration, melee, attack_speed.max(0.01));
}

/// The skill whose combo fields a hit gains by: a `Skill_WPAttack_BasicAttack`
/// gains by its weapon-pool parent, others by themselves.
pub fn gains_by_parent(rec: &Record) -> bool {
    rec.class == "Skill_WPAttack_BasicAttack"
}
