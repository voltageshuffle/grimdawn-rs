//! Binding hotbar slots: the HUD's slot pick list (`hudSlotSelectWindow`, a `SlotSelectWindow`). The UI shows
//! [`choices`] and sends [`BindSlot`].
//!
//! Pick list:
//! - a right press on a hot slot button runs `PlayerHotSlotCtrl::DeactivateActiveSlot` and, when the button
//!   allows it (off only for the two potion buttons and the evade button), opens the list at the button;
//! - the list (`HotSlotPickList::Fill`) is, in order: every skill of `Character::GetUISkillList` that passes
//!   `HotSlotButton::IsSkillSelectable` — current level ≠ 0, `IsPrimary` or `IsSecondary`, not `IsAutoToggle`
//!   ([`gd_sim::skills::hot_slot_flags`]), of the displayed skill set or (no parent skill and)
//!   `SkillManager::IsGlobalSkillTypeAndAllowed`, and on the left mouse slot (UI slot 10) only `IsPrimary` ones —
//!   as an evade / health potion / energy potion option when it is the `SkillManager` special skill 7 / 5 / 6,
//!   else a skill option; then, on every slot but the left mouse one, one scroll option per distinct
//!   `OneShot_Scroll` record in the bags (ordered by record name); then, on every slot but the left mouse one,
//!   the empty pick (`tagEmptyHotSlot`);
//! - `GetUISkillList`: the default player skills in `LoadDefaultPlayerSkills` order (`defaultwpbasicattack`,
//!   `defaultmoveto`, `defaultweaponattack`, `defaultpetattack`, the two potions, `defaultevade`; not the kick),
//!   then the skills as created (the save's order, then learned ones) and the item skills without an auto-cast
//!   controller;
//! - a pick (left or right press) sets the slot (UI slot 10 `SetPrimarySlot`, 11 `SetSecondarySlot`, else
//!   `SetHotSlot`), plays `skillSelectSound` and closes the list; a left press outside it closes it;
//! - the save: a skill option is type 0, the potions types 2 / 3, evade type 5, a scroll type 4 (item record,
//!   `bitmapButtonUp`, `bitmapButtonDown`, its text); a scroll slot uses one of that record from the bags.
//!
//! The armed slot ([`ArmedSlot`]):
//! - a left press on a hot slot button runs `PlayerHotSlotCtrl::ActivateHotSlot` (`fromMouse` set): the slot
//!   becomes the active one and its option is activated; `HotSlotOptionSkill::Activate` uses a skill of
//!   activation mode 1 / 2 at once at the player ([`gd_sim::skills::used_at_once`]), and for any other skill asks
//!   for a target: the slot stays armed. Potion, scroll and evade options never ask. A hot slot key goes the same
//!   way with `fromMouse` clear: the skill is requested at once at the cursor record and the slot is not armed
//!   ([`HotSlotKey`] → [`CastRequest`]); either way the previously armed slot is dropped;
//! - the armed slot is used by the next world press, left or right (`ActivateSuperSkill` takes and clears it:
//!   [`UseArmedSlot`]) or a press on a pet portrait ([`gd_game_core::casting::CastOnAlly`]); a refused request
//!   still spends it;
//! - it is dropped ([`DisarmHotSlot`]) by a right press on a hot slot button, Esc (taking the key only when a
//!   slot was armed) and a HUD window being shown — the only callers of `DeactivateActiveSlot` besides a press of
//!   another slot;
//! - the controller's cursor record ([`WorldCursor`], `ControllerPlayer.cursorObjectId` / `cursorCoords`) is
//!   written by `InGameUI::HandleMouseEvent` on every mouse event it gets (moves too) — events the UI took never
//!   reach it, so over the HUD the record keeps the last world point. It holds the object under the cursor (a
//!   foe, a friend, an NPC; its coordinates) or the ground point under the cursor (no ground hit: left as it
//!   was). A key press requests its skill at the record, and an evade option dashes towards it;
//! - a held hot slot key repeats ([`HeldHotSlots`]): the key's press presses the slot's button and, when the
//!   option `IsRepeatable` ([`gd_sim::skills::repeatable`]; a scroll's never), puts the slot at the front of the
//!   held list (not again when it is the front already), sets the lock and restarts its timer; the key's release
//!   takes every entry of that slot out. `UIPlayerHud::Update` clears the lock 50 ms after its timer's restart,
//!   empties the list while the player is not alive or teleporting, and with the lock clear, the list not empty,
//!   the player not moving and no new mouse command pending presses the front slot's button again (no cooldown /
//!   energy voice) and sets the lock again: at most one request per 50 ms, refused like any request while the
//!   previous cast runs;
//! - the look: no slot highlight; while armed the cursor is `ui/cursor/cursorreticle.tex`,
//!   `cursorreticleattack.tex` over an attack target (`InGameUI::UpdateCursor`).
//!
//! - the world mouse handler runs at least once a frame (`DirectInputDevice::Update` adds one move event per
//!   update), so the UI sends [`CursorOverWorld`] every frame the cursor is over the world, and [`WorldMouse`] for
//!   each press and each frame;
//! - the new mouse command ([`NewMouseCommand`], `ControllerPlayer.newMouseCommand`): set by a world press (left
//!   or right) and by the release of the last held hot slot key, cleared by a world mouse event with no left /
//!   right button held and by the primary action's item / NPC / fixed actor requests (in Keyboard Only only a
//!   picked NPC or item counts: [`WorldMouse::object`]); while set the held keys do not repeat;
//! - the teleport state (`Character::IsTeleporting`): [`crate::teleport::PendingTravel`].
//!
//! - no drag: `CursorHandlerHotSlotOption` is the only handler that accepts hot slots, and nothing creates it:
//!   skills and items are never dragged onto, off or between hot slots.
//!
//! PORT: a friend's record is its coordinates (the engine's is the ground point or the walk-to point by the
//! `targetLock` option).
//! PORT: a slot holds its option as a record ([`Caster::slots`]): a skill, [`HEALTH_POTION`] / [`ENERGY_POTION`] /
//! [`EVADE`] for those options, an `OneShot_Scroll` item record for a scroll ([`option_kind`]). The form skills
//! granted while shapeshifted are listed after the item skills (`AddSubSkills`' place was not traced). The saved
//! scroll text is left empty (the engine writes the item's name).
use crate::evade::{EVADE, EvadeRequest};
use bevy::prelude::*;
use gd_formats::vfs::Database;
use gd_game_core::casting::{CastOnAlly, CastRequest, ENERGY_POTION, HEALTH_POTION, UsePotion};
use gd_game_core::casting::{Caster, SLOT_LEFT, SLOTS};
use gd_game_core::data::GameData;
use gd_game_core::inventory::Inventory;
use gd_game_core::player::{Loadout, Player};

pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BindSlot>()
            .add_message::<ActivateHotSlot>()
            .add_message::<DisarmHotSlot>()
            .add_message::<UseArmedSlot>()
            .add_message::<CursorOverWorld>()
            .add_message::<crate::controller_target::ControllerTarget>()
            .add_message::<crate::controller_target::UseObject>()
            .add_message::<HotSlotKey>()
            .add_message::<ShowSkillSet>()
            .add_message::<WorldMouse>()
            .init_resource::<ArmedSlot>()
            .init_resource::<NewMouseCommand>()
            .init_resource::<WorldCursor>()
            .init_resource::<HeldHotSlots>()
            .add_systems(
                Update,
                (record_cursor, new_mouse_command)
                    .in_set(HotbarCursorSet)
                    .before(HotbarSet)
                    .before(crate::evade::EvadeStage::Start),
            )
            // Binds / potion / evade requests must land before casting reads them (UsePotion in CastingSet,
            // EvadeRequest before CombatSet).
            .add_systems(
                Update,
                (show_skill_set, hot_slot_keys, arming, use_options)
                    .chain()
                    .in_set(HotbarSet)
                    .after(gd_game_core::player::PlayerSet)
                    .before(crate::casting::CastingSet),
            );
    }
}

/// The hotbar's cursor readers: before the game's input writers that read the cursor record and [`HotbarSet`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HotbarCursorSet;

/// The hotbar's request chain: after the frame's input writers and before `CastingSet`, so a key press or a
/// right-click option becomes its request the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HotbarSet;

/// Another hotbar page shown: [`Caster::next_displayed_set`] / [`Caster::reset_displayed_set`].
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowSkillSet {
    Next,
    Reset,
}

fn show_skill_set(
    mut msgs: MessageReader<ShowSkillSet>,
    mut players: Query<&mut Caster, With<Player>>,
) {
    for m in msgs.read() {
        let Ok(mut caster) = players.single_mut() else {
            continue;
        };
        match m {
            ShowSkillSet::Next => caster.next_displayed_set(),
            ShowSkillSet::Reset => caster.reset_displayed_set(),
        }
    }
}

/// The Move skill (`SkillManager::GetDefaultSkillId(3)`). Its request always fails
/// (`Skill_Move::GetValidTarget` returns false), so on a mouse slot it only moves: [`move_slot_keeps_target`].
pub const MOVE_TO: &str = "records/skills/default/defaultmoveto.dbr";

pub fn is_move_to(skill: &str) -> bool {
    skill.eq_ignore_ascii_case(MOVE_TO)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressedObject {
    /// A `Player` or a `Monster` (foes, pets).
    Character,
    /// A `Destructible` (a breakable).
    Destructible,
    /// Anything else the pick gives (an NPC, a fixed actor...).
    Other,
}

/// A world mouse press (or hold) whose slot option is the Move skill (`InGameUI::HandleMouseEvent`): the object
/// under the cursor stays the command's target only for a left press on something that is not a character, and on
/// a breakable only with the option `breakingMoveTo` (default on). Otherwise the press is a plain move to the
/// cursor point: the target id is dropped, the button acts as the primary one (a right press with Move walks too)
/// and the command counts as repeated. A kept target goes to `SelectPrimaryAction`, whose Move request fails, so
/// the default weapon attack hits it: Move breaks breakables but never attacks a monster.
pub fn move_slot_keeps_target(
    right_button: bool,
    object: Option<PressedObject>,
    breaking_move_to: bool,
) -> bool {
    match object {
        _ if right_button => false,
        None | Some(PressedObject::Character) => false,
        Some(PressedObject::Destructible) => breaking_move_to,
        Some(PressedObject::Other) => true,
    }
}

/// [`move_slot_keeps_target`] for a picked breakable ([`PressedObject::Destructible`]) or anything else
/// ([`PressedObject::Character`]).
pub fn move_slot_keeps(right_button: bool, breakable: bool, breaking_move_to: bool) -> bool {
    move_slot_keeps_target(
        right_button,
        Some(if breakable {
            PressedObject::Destructible
        } else {
            PressedObject::Character
        }),
        breaking_move_to,
    )
}

/// Whether the primary (left-button) action on this `Destructible` record is the kick, whatever skill the left
/// slot holds: `SelectPrimaryAction` requests [`crate::casting::DEFAULT_KICK`] for a `Destructible` whose
/// `attackAnimationType` is 47 (`attackAnimation` exactly "Kick"). The right button and hot-slot keys have no such
/// branch. The kick is a melee attack with the record's `weaponDamagePct` 150 and flat physical 8–10.
pub fn kicks_breakable(rec: &gd_formats::arz::Record) -> bool {
    rec.str("attackAnimation").is_some_and(|a| a == "Kick")
}

/// The default player skills of the UI skill list, in its order.
pub const DEFAULT_SKILLS: [&str; 6] = [
    MOVE_TO,
    gd_game_core::casting::DEFAULT_ATTACK,
    "records/skills/default/defaultpetattack.dbr",
    HEALTH_POTION,
    ENERGY_POTION,
    EVADE,
];

/// The armed hot slot (`IsAnySlotActive`): clicked with the mouse, waiting for its target. Set and cleared by
/// [`arming`] only.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArmedSlot(pub Option<usize>);

/// The controller's cursor record: the character under the cursor and the world point at the last mouse event
/// over the world (in Keyboard Only the controller targeting's pick instead). Set by [`record_cursor`] only;
/// `point` None until the first one (and for an empty controller record).
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct WorldCursor {
    pub target: Option<Entity>,
    pub point: Option<Vec3>,
}

/// A mouse event over the world: the character under the cursor (a foe or one's own pet) and its coordinates,
/// else the ground point under the cursor (`SetMouseRepeatData`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct CursorOverWorld {
    pub target: Option<Entity>,
    pub point: Vec3,
}

/// [`CursorOverWorld`] or, in Keyboard Only, [`ControllerTarget`](crate::controller_target::ControllerTarget)
/// (which may empty the record) → [`WorldCursor`] (the last event of the frame wins).
fn record_cursor(
    mut events: MessageReader<CursorOverWorld>,
    mut controller: MessageReader<crate::controller_target::ControllerTarget>,
    mut cursor: ResMut<WorldCursor>,
) {
    if let Some(e) = events.read().last() {
        *cursor = WorldCursor {
            target: e.target,
            point: Some(e.point),
        };
    }
    if let Some(e) = controller.read().last() {
        *cursor = WorldCursor {
            target: e.target,
            point: e.point,
        };
    }
}

/// A mouse event that reached the world: a left / right press, and whether a left or right button is held. Sent
/// once per press and once per frame. `object`: the press is a primary action on an item, an NPC or a fixed
/// actor.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct WorldMouse {
    pub press: bool,
    pub held: bool,
    pub object: bool,
}

/// `ControllerPlayer::IsNewMouseCommand`: a world press is pending, so held hot slot keys do not repeat. Set by
/// [`new_mouse_command`] and [`hot_slot_keys`] only.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct NewMouseCommand(pub bool);

/// [`WorldMouse`] → [`NewMouseCommand`]: a press sets it, its primary action on an item / NPC / fixed actor or an
/// event with no button held clears it.
fn new_mouse_command(mut events: MessageReader<WorldMouse>, mut flag: ResMut<NewMouseCommand>) {
    for e in events.read() {
        if e.press {
            flag.0 = true;
        }
        if !e.held || e.object {
            flag.0 = false;
        }
    }
}

/// A hot slot key went down or up; `slot` is the caster slot the press used.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct HotSlotKey {
    pub slot: usize,
    pub down: bool,
}

/// `UIPlayerHud.heldHotSlots`: repeatable slots whose key is down, the most recent first, and the time of the
/// last press. Set by [`hot_slot_keys`] only.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct HeldHotSlots {
    pub slots: std::collections::VecDeque<usize>,
    pub last_press: Option<f32>,
}

/// `UIPlayerHud::Update`.
pub const REPEAT_LOCK: f32 = 0.05;

/// A press requests the slot's option at the cursor record and holds a repeatable slot; the update repeats the
/// front held slot once the lock is over and the player stands.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn hot_slot_keys(
    mut keys: MessageReader<HotSlotKey>,
    mut held: ResMut<HeldHotSlots>,
    mut out: MessageWriter<CastRequest>,
    time: Res<Time>,
    data: Res<GameData>,
    cursor: Res<WorldCursor>,
    players: Query<
        (
            &Transform,
            &Caster,
            &crate::movement::MovePath,
            Has<gd_game_core::combat::Dead>,
        ),
        With<Player>,
    >,
    alive: Query<(), Without<gd_game_core::combat::Dead>>,
    options: Option<Res<gd_game_core::options::GameOptions>>,
    (mut new_mouse, travel): (
        ResMut<NewMouseCommand>,
        Option<Res<crate::teleport::PendingTravel>>,
    ),
) {
    let now = time.elapsed_secs();
    let Ok((tf, caster, path, dead)) = players.single() else {
        keys.clear();
        held.slots.clear();
        return;
    };
    // An empty record: in the keyboard movement modes the player's own coordinates. PORT: in Classic a point 5 m
    // ahead (the engine's request with an empty point was not traced).
    let keyboard = options.as_ref().is_some_and(|o| o.int("movementType") > 0);
    let empty = if keyboard {
        tf.translation
    } else {
        tf.translation + tf.rotation * Vec3::Z * 5.0
    };
    // `RequestSkillAction` at the controller's cursor record.
    let request = |slot: usize| CastRequest {
        slot,
        target: cursor.target.filter(|t| alive.get(*t).is_ok()),
        point: cursor.point.unwrap_or(empty),
        force: false,
        scripted: false,
    };
    for k in keys.read() {
        if !k.down {
            held.slots.retain(|s| *s != k.slot);
            // `UIPlayerHud::HotSlotKeyUp`.
            if held.slots.is_empty() {
                new_mouse.0 = true;
            }
            continue;
        }
        let Some(option) = caster.slots.get(k.slot).cloned().flatten() else {
            continue;
        };
        out.write(request(k.slot));
        let repeatable = option_kind(&data.db, &option) != OptionKind::Scroll
            && data
                .db
                .get(&option)
                .is_some_and(|r| gd_sim::skills::repeatable(&r.class));
        if repeatable {
            if held.slots.front() != Some(&k.slot) {
                held.slots.push_front(k.slot);
            }
            held.last_press = Some(now);
        }
    }
    if held.last_press.is_some_and(|t| now - t >= REPEAT_LOCK) {
        held.last_press = None;
    }
    if dead || travel.is_some_and(|t| t.0.is_some()) {
        held.slots.clear();
    }
    if held.last_press.is_none()
        && path.points.is_empty()
        && !new_mouse.0
        && let Some(&slot) = held.slots.front()
    {
        if caster.slots.get(slot).is_some_and(|o| o.is_some()) {
            out.write(request(slot));
        }
        held.last_press = Some(now);
    }
}

/// A left press on a hot slot's button (`ActivateHotSlot` with `fromMouse`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ActivateHotSlot {
    pub slot: usize,
}

/// Drop the armed slot (`PlayerHotSlotCtrl::DeactivateActiveSlot`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct DisarmHotSlot;

/// A world press with a slot armed: use it at `target` (a foe or one's own pet) or the ground `point`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct UseArmedSlot {
    pub target: Option<Entity>,
    pub point: Vec3,
}

/// Every [`CastRequest`] drops the armed slot, [`DisarmHotSlot`] drops it, an [`ActivateHotSlot`] uses its option
/// at once or arms it, [`UseArmedSlot`] / [`CastOnAlly`] take it and request its skill.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn arming(
    mut armed: ResMut<ArmedSlot>,
    mut presses: MessageReader<ActivateHotSlot>,
    mut disarms: MessageReader<DisarmHotSlot>,
    mut uses: MessageReader<UseArmedSlot>,
    mut allies: MessageReader<CastOnAlly>,
    mut requests: ParamSet<(MessageReader<CastRequest>, MessageWriter<CastRequest>)>,
    data: Res<GameData>,
    players: Query<(Entity, &Transform, &Caster), With<Player>>,
    pets: Query<
        (&Transform, &crate::pets::Pet),
        (Without<Player>, Without<gd_game_core::combat::Dead>),
    >,
) {
    let used = requests.p0().read().count() > 0;
    if used | (disarms.read().count() > 0) {
        armed.0 = None;
    }
    let Ok((me, tf, caster)) = players.single() else {
        armed.0 = None;
        uses.clear();
        allies.clear();
        presses.clear();
        return;
    };
    let mut out: Vec<CastRequest> = Vec::new();
    let at = |slot: usize, target: Option<Entity>, point: Vec3| CastRequest {
        slot,
        target,
        point,
        force: false,
        scripted: false,
    };
    for p in presses.read() {
        armed.0 = None;
        let Some(option) = caster.slots.get(p.slot).cloned().flatten() else {
            continue;
        };
        let now = option_kind(&data.db, &option) != OptionKind::Skill
            || data.db.get(&option).is_some_and(|r| {
                gd_sim::skills::used_at_once(&r.class, r.bool("instantCast"), r.bool("pointBlank"))
            });
        if now {
            // At the player; an evade option aims by the cursor record itself.
            out.push(at(p.slot, None, tf.translation + tf.rotation * Vec3::Z));
        } else {
            debug!("hot slot {} armed", p.slot);
            armed.0 = Some(p.slot);
        }
    }
    for u in uses.read() {
        if let Some(slot) = armed.0.take() {
            out.push(at(slot, u.target, u.point));
        }
    }
    for a in allies.read() {
        // The portrait: the pet as the combat ally, its coordinates; only one's own living pet.
        let Some((ptf, _)) = pets.get(a.ally).ok().filter(|(_, p)| p.owner == me) else {
            continue;
        };
        if let Some(slot) = armed.0.take() {
            out.push(at(slot, Some(a.ally), ptf.translation));
        }
    }
    let mut w = requests.p1();
    for r in out {
        w.write(r);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    Skill,
    HealthPotion,
    EnergyPotion,
    Evade,
    Scroll,
}

pub fn option_kind(db: &Database, rec: &str) -> OptionKind {
    if rec.eq_ignore_ascii_case(HEALTH_POTION) {
        OptionKind::HealthPotion
    } else if rec.eq_ignore_ascii_case(ENERGY_POTION) {
        OptionKind::EnergyPotion
    } else if rec.eq_ignore_ascii_case(EVADE) {
        OptionKind::Evade
    } else if db.get(rec).is_some_and(|r| r.class == "OneShot_Scroll") {
        OptionKind::Scroll
    } else {
        OptionKind::Skill
    }
}

/// Bind `option` (None = empty) to the shown hotbar page's `slot`.
#[derive(Message, Clone, Debug)]
pub struct BindSlot {
    pub slot: usize,
    pub option: Option<String>,
}

/// The pick list of `slot`, `None` = the empty pick.
pub fn choices(
    db: &Database,
    loadout: &Loadout,
    effective: &gd_sim::skills::SkillBook,
    caster: &Caster,
    inv: Option<&Inventory>,
    slot: usize,
) -> Vec<Option<String>> {
    let primary_slot = slot == SLOT_LEFT;
    let item_skills = effective
        .item_grants
        .iter()
        .filter(|g| g.controller.is_none())
        .map(|g| g.skill.clone());
    let learned = loadout
        .skills
        .skills
        .iter()
        .map(|(n, _)| n.clone())
        .filter(|n| !gd_game_core::player::is_default_skill(n));
    let all = DEFAULT_SKILLS
        .iter()
        .map(|s| s.to_string())
        .chain(learned)
        .chain(item_skills)
        .chain(loadout.granted.iter().map(|g| g.0.clone()));
    let mut out: Vec<Option<String>> = Vec::new();
    for sk in all {
        if out.iter().flatten().any(|o| o.eq_ignore_ascii_case(&sk)) {
            continue;
        }
        let Some(rec) = db.get(&sk) else { continue };
        let (primary, secondary, auto) = gd_sim::skills::hot_slot_flags(&rec.class);
        if !(primary || secondary)
            || auto
            || (primary_slot && !primary)
            || loadout.skill_level(db, effective, &sk) == 0
        {
            continue;
        }
        let set_ok = rec.i32("skillSet").unwrap_or(0) == caster.shown
            || loadout.form.as_ref().is_none_or(|(_, allowed)| {
                allowed.is_empty() || allowed.iter().any(|a| a.eq_ignore_ascii_case(&rec.class))
            });
        if set_ok {
            out.push(Some(sk));
        }
    }
    if primary_slot {
        return out;
    }
    if let Some(inv) = inv {
        let mut scrolls: Vec<String> = inv
            .bags
            .iter()
            .flat_map(|b| &b.items)
            .map(|p| &p.item.base_name)
            .filter(|n| db.get(n).is_some_and(|r| r.class == "OneShot_Scroll"))
            .cloned()
            .collect();
        scrolls.sort();
        scrolls.dedup();
        out.extend(scrolls.into_iter().map(Some));
    }
    out.push(None);
    out
}

/// Binds the requested options; one that is neither a fixed option nor a database record is refused.
fn bind_slots(
    db: &Database,
    requests: &mut MessageReader<BindSlot>,
    players: &mut Query<&mut Caster, With<Player>>,
) {
    for req in requests.read() {
        let Ok(mut caster) = players.single_mut() else {
            continue;
        };
        if req.slot >= SLOTS {
            continue;
        }
        if let Some(o) = &req.option
            && option_kind(db, o) == OptionKind::Skill
            && db.get(o).is_none()
        {
            warn!("hotbar slot {}: unknown option {o}", req.slot);
            continue;
        }
        info!(
            "hotbar slot {}: {:?}",
            req.slot,
            req.option
                .as_deref()
                .map(|s| s.rsplit('/').next().unwrap_or(s))
        );
        caster.slots[req.slot] = req.option.clone();
    }
}

/// [`BindSlot`]s; a used slot holding a potion, evade or scroll option does what the R / E keys, Space or a
/// right click on the item in the bags do.
#[allow(clippy::too_many_arguments)]
fn use_options(
    mut binds: MessageReader<BindSlot>,
    mut requests: MessageReader<CastRequest>,
    data: Res<GameData>,
    mut players: Query<&mut Caster, With<Player>>,
    // Option: binds and potion / evade options work without an inventory; only the scroll option needs it.
    mut inv: Option<ResMut<Inventory>>,
    mut potions: MessageWriter<UsePotion>,
    mut evades: MessageWriter<EvadeRequest>,
) {
    bind_slots(&data.db, &mut binds, &mut players);
    for req in requests.read() {
        let Ok(caster) = players.single() else {
            continue;
        };
        let Some(rec) = caster.slots.get(req.slot).cloned().flatten() else {
            continue;
        };
        match option_kind(&data.db, &rec) {
            OptionKind::Skill => {}
            OptionKind::HealthPotion => {
                potions.write(UsePotion::Health);
            }
            OptionKind::EnergyPotion => {
                potions.write(UsePotion::Energy);
            }
            OptionKind::Evade => {
                evades.write(EvadeRequest {
                    scripted: req.scripted,
                });
            }
            OptionKind::Scroll => {
                let Some(inv) = inv.as_mut() else { continue };
                // Not while the scroll cools down (the use would be refused anyway).
                let cooling = data.db.get(&rec).is_some_and(|r| {
                    caster
                        .cooldowns
                        .contains_key(&crate::consumables::scroll_key(r, &rec))
                });
                if !cooling && inv.used.is_empty() && inv.remove_by_record(&rec, 1) == 1 {
                    inv.used.push((
                        gd_formats::save::item::Item {
                            base_name: rec,
                            stack_count: 1,
                            ..Default::default()
                        },
                        None,
                    ));
                }
            }
        }
    }
}
