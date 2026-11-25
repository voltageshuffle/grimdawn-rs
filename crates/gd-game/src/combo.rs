//! The player's combo charges (rules and engine addresses: `gd_sim::combo`).
//!
//! [`ComboUse`] is the skill use that may spend them, reset when a cast starts.
//! `combat::apply_hits` scales a use's hits by the multiplier taken on its first hit, spends after
//! that hit and gains charges from every skill hit. Charges expire ([`tick`]) and clear on a weapon
//! change, the swap buttons and riftgate / portal travel ([`clear`]).
//!
//! PORT: the gain runs per hit message, not once per skill target result (the lockout makes them
//! equal); a use spends on its first hit only; the lockout's melee test is "weapon skill without a
//! ranged weapon" for `AttackInfo.isMelee`; "player teleport" is riftgate / portal travel. The
//! count's aura (`skillComboChargeAura`) is a buff effect on the player; the "remain" effect /
//! sound of a kept spend is not played.
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::data::GameData;
use gd_game_core::inventory::Inventory;
use gd_game_core::player::Player;
use gd_rules::Rng;
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
                .before(gd_game_core::combat::CombatSet)
                .after(gd_game_core::inventory::InventorySet),
        );
    }
}

#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct Combo(pub ComboCharges);

#[derive(Resource, Debug, Clone, Default)]
pub struct ComboRules(pub ComboTable);

/// `Skill.comboClaim` / `Skill.comboClaimCount`.
#[derive(Component, Debug, Clone, Default)]
pub struct ComboUse {
    /// The data record its hits name.
    pub skill: String,
    /// Has a cooldown and is not channelled.
    pub eligible: bool,
    pub multiplier: Option<f32>,
}

impl ComboUse {
    /// `rec`: the cast's data record; `cooldown` in s.
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
    let ms = gd_game_core::session::frame_ms(&mut carry, &time);
    if ms <= 0 {
        return;
    }
    for mut c in &mut q {
        if c.0.remaining_ms > 0 || c.0.lockout_ms > 0 {
            c.0.update(ms);
        }
    }
}

/// `ClearComboCharges`: a weapon change, the swap buttons, travel.
fn clear(
    mut fresh: gd_game_core::session::NewSession,
    // Option: travel clears the combo without an inventory.
    inv: Option<Res<Inventory>>,
    mut swaps: MessageReader<crate::weapon_sets::SwapWeapons>,
    mut travel: MessageReader<crate::riftgates::GateTravel>,
    mut q: Query<&mut Combo, (With<Player>, Without<gd_game_core::combat::Dead>)>,
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

#[derive(Component, Debug, Clone, Default)]
pub struct ComboAura(pub Option<String>);

/// `SetComboChargeEffect`.
fn look(
    mut commands: Commands,
    data: Res<GameData>,
    rules: Res<ComboRules>,
    q: Query<(Entity, &Combo, Option<&ComboAura>), Changed<Combo>>,
    fx: Query<(Entity, &gd_game_core::visuals::BuffFx)>,
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

#[derive(Debug, Clone, Copy, Default)]
pub struct HitCombo {
    /// (100 + multiplier) %, 0 = none.
    pub scale: f32,
    pub spends: bool,
}

/// `CollectComboMultiplier`.
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

/// After a hit: `SpendComboCharges` (`reduction`: `skillComboChargeSpendReduction`), then
/// `IncrementComboCharge` from the gaining skill and its modifiers at their levels (`gain`).
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
