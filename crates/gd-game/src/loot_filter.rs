//! The loot filter's options (rules: `gd_sim::items::loot_filter`).
//!
//! [`LootFilter`] holds the 42 options of the character, read from its
//! file (`CharacterInfo` `loot_filters`, like the real game, which keeps them
//! per character: `Player::GetLootFilter`) or a new character's defaults,
//! and written back with it (`save.rs`, the port's own save folder). The
//! loot filter window sends [`LootFilterRequest`]s; the game reads
//! [`LootFilter::passes`] for each ground item's label and pick.
//!
//! The `ItemIgnore` mode each reader passes (`Item::PassLootFilter`, modes
//! [`Mode`]) comes from the held keys 35 "Show Items (No Filter)" / 37 "Show Items (Filter Common)"
//! and the HUD's "Toggle Hide All Items" flag (UI state, `grimdawn::loot`):
//! - the ground label and loot beam: [`label_mode`] (GrimDawn.exe `UIWorldDescManager::ApplyLootFilter`);
//! - the cursor pick and the Pickup key: [`pick_mode`] (GrimDawn.exe `InGameUI::GetPickFilterMode`), which
//!   adds the `lockItemPickup` option: off, every item but Broken ones can be picked unless
//!   key 37 is held. Its callers: the world mouse handler `InGameUI::HandleMouseEvent` (cursor pick,
//!   `grimdawn::loot`), the HUD's Pickup key `UIPlayerHud::PickUpNearest` ([`PickUpNearest`]) and the controller
//!   targeting of `InGameUI::Update` (stored in `InGameUI.controllerTarget` for
//!   `InGameUI::CollectEnemyCandidates`; PORT: the port has no
//!   controller targeting).
//!
//! [`PickUpNearest`], key action 55 "Pickup" (`tagPickup`, no default key), GrimDawn.exe
//! `UIPlayerHud::PickUpNearest` (HUD key handler `UIPlayerHud::HandleKeyAction`): of the items within the 10 m
//! sphere around the
//! player, the nearest whose distance is at most the player's radius + the item's radius + 2 m and
//! which passes the filter in the pick mode; the player controller is ordered to pick it up (walks
//! over: [`crate::player_orders::InteractTarget`]). The item's radius is `Actor::GetRadius`: the length of
//! its scale vector (`Actor::Load` passes the record's `scale` three times to `SetScale`) x `actorRadius`
//! ([`item_radius`]). The engine also skips an item that is not of interest (`Item::IsOfInterest`:
//! the record's `cannotPickUp` is clear and `SetPickupOk` was called with true: PORT: a [`crate::loot::Tossed`]
//! item is pickable once released, like the cursor pick) or not visible (a thrown item waiting for its release
//! is hidden), one the player already has that cannot be picked up twice (`Character::HasItem` by record name
//! && `cannotPickUpMultiple`, [`Inventory::has_record`]) and one that belongs to another player (single player
//! here). PORT: a gold pile's radius is 0 (its record is not kept).
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::inventory::Inventory;
use gd_game_core::player::{Loadout, Player};
use gd_sim::items::loot_filter::{COUNT, ItemFacts, Masteries, Mode, defaults, passes};

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct LootFilter {
    pub options: [bool; COUNT],
}

impl LootFilter {
    /// From a character file's flags (missing ones take the defaults).
    pub fn from_save(flags: &[u8]) -> Self {
        let d = defaults();
        Self {
            options: std::array::from_fn(|i| flags.get(i).map_or(d[i], |f| *f != 0)),
        }
    }

    /// The flags as the character file stores them.
    pub fn to_save(&self) -> Vec<u8> {
        self.options.iter().map(|b| *b as u8).collect()
    }

    /// Whether `item` gets a label in `mode` for a player with `loadout`.
    pub fn passes(
        &self,
        inv: &Inventory,
        db: &gd_formats::vfs::Database,
        item: &Item,
        mode: Mode,
        loadout: Option<&Loadout>,
    ) -> bool {
        let Some(info) = inv.info(item) else {
            return true;
        };
        let r = &info.resolved;
        let rec = |n: &str| (!n.is_empty()).then(|| db.get(n)).flatten();
        let pet_bonus = [&item.base_name, &item.prefix_name, &item.suffix_name]
            .iter()
            .any(|n| rec(n).is_some_and(|r| r.str("petBonusName").is_some_and(|p| !p.is_empty())));
        // The skill bonuses leave out the inserted relic and augment: an item without either is read as is.
        let skills = if item.relic_name.is_empty()
            && item.relic_bonus.is_empty()
            && item.augment_name.is_empty()
        {
            gd_sim::skills::item_skill_bonuses(db, item)
        } else {
            gd_sim::skills::item_skill_bonuses(
                db,
                &Item {
                    relic_name: String::new(),
                    relic_bonus: String::new(),
                    augment_name: String::new(),
                    ..item.clone()
                },
            )
        };
        let masteries: Vec<(String, bool)> = loadout
            .map(|l| {
                l.skills
                    .skills
                    .iter()
                    .filter(|(n, _)| n.to_ascii_lowercase().contains("_classtraining_"))
                    .map(|(n, lv)| (n.to_ascii_lowercase(), *lv > 0))
                    .collect()
            })
            .unwrap_or_default();
        passes(
            &self.options,
            mode,
            &ItemFacts {
                resolved: r,
                pet_bonus,
                skills: &skills,
            },
            &Masteries(&masteries),
        )
    }
}

/// The ground label's mode (module doc; GrimDawn.exe `UIWorldDescManager::ApplyLootFilter`): key 37 held wins
/// over key 35, then the hide-all toggle.
pub fn label_mode(show_all: bool, hide_common: bool, items_hidden: bool) -> Mode {
    if hide_common {
        Mode::HideCommon
    } else if show_all {
        Mode::ShowAll
    } else if items_hidden {
        Mode::HideAll
    } else {
        Mode::Filter
    }
}

/// The pick mode (module doc; GrimDawn.exe `InGameUI::GetPickFilterMode`): `lockItemPickup` on, the
/// label's mode; off, `ShowAll`; the held keys override either way (37 over 35).
pub fn pick_mode(
    lock_item_pickup: bool,
    show_all: bool,
    hide_common: bool,
    items_hidden: bool,
) -> Mode {
    if lock_item_pickup || hide_common || show_all {
        label_mode(show_all, hide_common, items_hidden)
    } else {
        Mode::ShowAll
    }
}

/// The Pickup key (module doc), with the [`pick_mode`] of the moment.
#[derive(Message, Clone, Copy, Debug)]
pub struct PickUpNearest(pub Mode);

/// The engine's search sphere around the player (`World::GetEntitiesInSphere`, 10 m).
const PICKUP_SPHERE: f32 = 10.0;

/// A ground item's `Actor::GetRadius` (module doc) from its base record: [`gd_game_core::combat::get_radius`] with the
/// engine's `actorRadius` default (a missing `scale` is 1, `Actor::Load`).
pub fn item_radius(rec: &gd_formats::arz::Record) -> f32 {
    gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS)
}

/// Walks the player over to the nearest item the filter lets it pick (module doc).
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn pick_up_nearest(
    mut requests: MessageReader<PickUpNearest>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    filter: Res<LootFilter>,
    inv: Option<Res<Inventory>>,
    data: Res<gd_game_core::data::GameData>,
    nav: Res<crate::nav::Nav>,
    mut players: Query<
        (
            &Transform,
            &mut gd_game_core::combat::Combatant,
            Option<&Loadout>,
            &mut crate::player_orders::InteractTarget,
            &mut crate::movement::MovePath,
        ),
        With<Player>,
    >,
    items: Query<(
        Entity,
        &GlobalTransform,
        &crate::loot::GroundItem,
        Option<&crate::loot::Tossed>,
    )>,
) {
    let Some(&PickUpNearest(mode)) = requests.read().last() else {
        return;
    };
    let Ok((tf, mut cb, loadout, mut target, mut path)) = players.single_mut() else {
        return;
    };
    let mut best: Option<(Entity, Vec3, f32)> = None;
    for (e, gt, gi, toss) in &items {
        let p = gt.translation();
        let d = p.distance(tf.translation);
        if d > PICKUP_SPHERE || best.is_some_and(|b| d >= b.2) {
            continue;
        }
        if toss.is_some_and(|t| t.wait > 0.0) {
            continue;
        }
        let rec = gi.item.as_ref().and_then(|it| data.db.get(&it.base_name));
        if d > cb.radius + rec.map_or(0.0, item_radius) + 2.0 {
            continue;
        }
        if let (Some(rec), Some(it)) = (&rec, &gi.item)
            && (rec.bool("cannotPickUp")
                || rec.bool("cannotPickUpMultiple")
                    && inv.as_ref().is_some_and(|i| i.has_record(&it.base_name)))
        {
            continue;
        }
        let pass = match (&gi.item, inv.as_deref()) {
            (Some(it), Some(inv)) => filter.passes(inv, &data.db, it, mode, loadout),
            _ => true,
        };
        if pass {
            best = Some((e, p, d));
        }
    }
    let Some((e, p, _)) = best else { return };
    cb.target = None;
    target.0 = Some(e);
    if let Some(pts) = nav.0.find_path(
        &mut scratch,
        tf.translation.to_array(),
        p.to_array(),
        crate::nav::PATH_BUDGET_LONG,
    ) {
        path.points = pts.into_iter().skip(1).map(Vec3::from).collect();
    }
}

/// The window's checkboxes and its Defaults button.
#[derive(Message, Clone, Copy, Debug)]
pub enum LootFilterRequest {
    Set(usize, bool),
    Defaults,
}

pub fn init_loot_filter(
    mut commands: Commands,
    // Option: `have` is the existence check (this system creates the filter); `loaded` is absent for a new character.
    have: Option<Res<LootFilter>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    players: Query<(), With<Player>>,
) {
    if have.is_some() || players.is_empty() {
        return;
    }
    let f = match &loaded {
        Some(c) => LootFilter::from_save(&c.0.info.loot_filters),
        None => LootFilter {
            options: defaults(),
        },
    };
    info!(
        "loot filter: {} of {COUNT} options on",
        f.options.iter().filter(|b| **b).count()
    );
    commands.insert_resource(f);
}

pub fn apply_requests(mut requests: MessageReader<LootFilterRequest>, mut f: ResMut<LootFilter>) {
    for r in requests.read() {
        match *r {
            LootFilterRequest::Set(i, on) if i < COUNT => f.options[i] = on,
            LootFilterRequest::Set(..) => {}
            LootFilterRequest::Defaults => f.options = defaults(),
        }
    }
}

pub struct LootFilterPlugin;

impl Plugin for LootFilterPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LootFilterRequest>()
            .add_message::<PickUpNearest>()
            .add_systems(
                Update,
                (
                    init_loot_filter,
                    apply_requests
                        .in_set(LootFilterStage::Apply)
                        .run_if(resource_exists::<LootFilter>),
                )
                    .chain()
                    .after(gd_game_core::inventory::InventorySet),
            )
            .add_systems(
                Update,
                pick_up_nearest
                    .after(LootFilterStage::Apply)
                    .after(gd_game_core::player::PlayerSet)
                    .before(crate::loot::pick_up)
                    .run_if(resource_exists::<LootFilter>),
            );
    }
}

/// Loot filter requests applied: what reads the filter orders after `Apply`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LootFilterStage {
    Apply,
}
