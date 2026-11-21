//! Binding hotbar slots: the HUD's slot pick list (`hudSlotSelectWindow`,
//! `records/ui/hud/hud_hotslotselectmenu.dbr`, a `SlotSelectWindow`). The UI
//! (`grimdawn` `ui/hotbar_pick.rs`) shows [`choices`] and sends [`BindSlot`].
//!
//! `Grim Dawn.exe`:
//! - a right press on a hot slot button (`sub_1401E69C0`, event 2) runs
//!   `PlayerHotSlotCtrl::DeactivateActiveSlot` and, when the button allows
//!   it (+2224: off only for the two potion buttons and the evade button,
//!   `sub_140205BE0`), opens the list at the button (`sub_14021BEA0`);
//! - the list (`sub_140270420`) is, in order: every skill of
//!   `Character::GetUISkillList` that passes `sub_1401E79D0` — current level
//!   ≠ 0, `IsPrimary` or `IsSecondary`, not `IsAutoToggle`
//!   ([`gd_sim::skills::hot_slot_flags`]), of the displayed skill set or
//!   (no parent skill and) `SkillManager::IsGlobalSkillTypeAndAllowed`, and
//!   on the left mouse slot (UI slot 10) only `IsPrimary` ones — as an
//!   evade / health potion / energy potion option when it is the
//!   `SkillManager` special skill 7 / 5 / 6, else a skill option; then,
//!   on every slot but the left mouse one (`sub_1401E7B50`), one scroll
//!   option per distinct `OneShot_Scroll` record in the bags
//!   (`PlayerInventoryCtrl::GetUniquePotionsOfType(4)`: the potion map,
//!   ordered by record name) that validates (some left); then, on every
//!   slot but the left mouse one, the empty pick (`tagEmptyHotSlot`);
//! - `GetUISkillList` (Game.dll `SkillManager` +368): the default player
//!   skills in `LoadDefaultPlayerSkills` order (`defaultwpbasicattack`,
//!   `defaultmoveto`, `defaultweaponattack`, `defaultpetattack`, the two
//!   potions, `defaultevade`; the kick is not added), then the skills as
//!   created (`CreateAndLoadSkill`: the save's order, then learned ones) and
//!   the item skills (`AddItemSkills`, those without an auto-cast
//!   controller);
//! - a pick (left or right press, `sub_14026F8F0`) sets the slot
//!   (`sub_1401E80E0`: UI slot 10 `SetPrimarySlot`, 11 `SetSecondarySlot`,
//!   else `SetHotSlot`), plays `skillSelectSound` and closes the list; a
//!   left press outside it closes it.
//! - the save: a skill option is a `HotSlotOptionSkill` (type 0), the
//!   potions `HotSlotOptionPotionSkill` (types 2 / 3), evade type 5, a
//!   scroll `HotSlotOptionScroll` (type 4: item record, `bitmapButtonUp`,
//!   `bitmapButtonDown`, its text; `WriteProperties` @0x18031b9b0);
//!   a scroll slot uses one of that record from the bags
//!   (`HotSlotOptionScroll::Activate` → `UsePotionOfType`).
//!
//!
//! The armed slot ([`ArmedSlot`]), `Grim Dawn.exe` / Game.dll:
//! - a left press on a hot slot button (the button's listener sub_1401E79A0 →
//!   sub_1401E7EA0, `fromMouse` set) runs `PlayerHotSlotCtrl::ActivateHotSlot`
//!   @0x1803ed6a0: the slot becomes the active one (`SkillSet` +0) and its option
//!   is activated; `HotSlotOptionSkill::Activate` @0x18031bbc0 uses a skill of
//!   activation mode 1 / 2 at once at the player
//!   ([`gd_sim::skills::used_at_once`]), and for any other skill asks for a
//!   target: the slot stays armed. Potion, scroll and evade options never ask
//!   (their `Activate` clears the flag). A hot slot key goes the same way with
//!   `fromMouse` clear (sub_1402110D0 → sub_140210DB0 → sub_1401E7C60): the skill
//!   is requested at once at the cursor record and the slot is not armed (the
//!   port's [`HotSlotKey`] → [`CastRequest`]); either way the previously armed slot is dropped;
//! - the armed slot is used by the next world press, left or right
//!   (sub_140021000: `GetActiveSlotOption` replaces the mouse button's option,
//!   `HandleActionFromMouse` → `ControllerPlayerState::ActivateSuperSkill`
//!   takes and clears it: [`UseArmedSlot`]) or a press on a pet portrait
//!   ([`crate::casting::CastOnAlly`]); a refused request still spends it;
//! - it is dropped ([`DisarmHotSlot`]) by a right press on a hot slot button
//!   (sub_1401E69C0, before the pick list opens), Esc (the HUD's Esc handler
//!   sub_140211810 asks every slot button; the primary slot's, sub_1401E83F0, runs
//!   `DeactivateActiveSlot` and takes the key only when a slot was armed) and a
//!   HUD window being shown (sub_1402132A0 → sub_140219DA0; the list: grimdawn
//!   `ui/hotbar_pick.rs` `DisarmingWindows`) — the only callers of
//!   `DeactivateActiveSlot` besides a press of another slot;
//! - the controller's cursor record ([`WorldCursor`], `ControllerPlayer` +1084
//!   object id, +1088 coordinates: `SetMouseRepeatData` /
//!   `GetMouseRepeatData`) is written by the world's mouse handler
//!   sub_140021000 on every mouse event it gets (moves too: an event with no
//!   button held still runs the pick) — events the UI took never reach it, so
//!   over the HUD the record keeps the last world point. It holds the object
//!   under the cursor (`sub_14002FF80` pick: a foe, a friend, an NPC; its
//!   coordinates) or the ground point under the cursor (`sub_14002DDC0`; no
//!   ground hit: the record is left as it was). A key press requests its skill
//!   at the record (`HotSlotOptionSkill::Activate` → `RequestSkillAction`),
//!   and an evade option dashes towards it (`HotSlotOptionEvade::Activate`
//!   @0x180276840, [`crate::evade`]);
//! - a held hot slot key repeats ([`HeldHotSlots`], `Grim Dawn.exe`):
//!   the key's press (sub_1402110D0 → sub_140210DB0) presses the slot's button
//!   (sub_1401E7C60 → `ActivateHotSlot`, `fromMouse` clear) and, when the
//!   option `IsRepeatable` ([`gd_sim::skills::repeatable`]; a scroll's never),
//!   puts the slot at the front of the held list (HUD +29296, not again when
//!   it is the front already), sets the lock (+29336) and restarts its timer
//!   (+29344); the key's release (sub_140210EB0) takes every entry of that slot
//!   out. The HUD's update (sub_14020D550) clears the lock 50 ms after its
//!   timer's restart, empties the list while the player is not alive
//!   (`GetLifeState` ≠ 2) or teleporting, and with the lock clear, the list not
//!   empty, the player not moving (`Character::IsMoving`) and no new mouse
//!   command pending (`ControllerPlayer::IsNewMouseCommand`) presses the front
//!   slot's button again (no cooldown / energy voice: sub_1401E7C60's third
//!   argument) and sets the lock again: at most one request per 50 ms, the
//!   most recently pressed held key, refused like any request while the
//!   previous cast runs;
//! - the look: no slot highlight (the slot button's highlights are the option
//!   statuses of `HotSlotOptionSkill::GetStatus`: energy, unusable); while armed
//!   the cursor is `ui/cursor/cursorreticle.tex`, `cursorreticleattack.tex` over
//!   an attack target (sub_14002D920, `IsAnyActiveTargetingNeeded`).
//!
//! - the world mouse handler runs at least once a frame: the input device adds one move event (the cursor and the
//!   held buttons) to every update after its buffered button and wheel events (DirectInput.dll
//!   `DirectInputDevice::Update` @0x1800031a0), so the UI sends [`CursorOverWorld`] every frame the cursor is over
//!   the world, and [`WorldMouse`] for each press and each frame;
//! - the new mouse command ([`NewMouseCommand`], `ControllerPlayer` +1073, `Grim Dawn.exe` sub_140021000,
//!   sub_140210EB0, Game.dll `SelectPrimaryAction`): set by a world press (left or right) and by the release of the
//!   last held hot slot key, cleared by a world mouse event with no left / right button held (so every frame
//!   after the buttons are let go) and by the primary action's item / NPC / fixed actor requests (a left press on
//!   a ground label, on an available NPC or a fixed actor of interest, `SelectPrimaryAction` @0x180161d00 after
//!   the Item / Npc / FixedActor request; the label click sub_14028AC60 goes there too; in Keyboard Only the
//!   press acts on the cursor record, so only a picked NPC or item counts: [`WorldMouse::object`]); while set the
//!   held keys do not repeat (a held world button owns the player);
//! - the teleport state (`Character::IsTeleporting`): [`crate::teleport::PendingTravel`].
//!
//! - no drag: the hot slot button's mouse handler (`Grim Dawn.exe` sub_1401E69C0) assigns a slot by drop
//!   only when the cursor's held handler (game client +264, set by sub_1401EDAD0) accepts hot slots (vtable +40) and
//!   gives its option (+232: `GAME::CursorHandlerHotSlotOption::GetOption`, Game.dll); that handler is the only one
//!   whose +40 is true (the item handlers' return false), and nothing creates it (its constructors are only
//!   exported, the exe does not import them): skills and items are never dragged onto, off or between hot slots.
//!
//! PORT: a friend's record is its coordinates (the engine's is the ground point or the walk-to point by option
//! bool 27).
//! PORT: a slot holds its option as a record ([`Caster::slots`]): a skill,
//! [`HEALTH_POTION`] / [`ENERGY_POTION`] / [`EVADE`] for those options, an
//! `OneShot_Scroll` item record for a scroll ([`option_kind`]). The form
//! skills granted while shapeshifted are listed after the item skills (their
//! place in the engine list, `AddSubSkills`, was not traced). The saved
//! scroll text is left empty (the engine writes the item's name).
use crate::casting::{
    CastOnAlly, CastRequest, Caster, ENERGY_POTION, HEALTH_POTION, SLOT_LEFT, SLOTS, UsePotion,
};
use crate::data::GameData;
use crate::evade::{EVADE, EvadeRequest};
use crate::inventory::Inventory;
use crate::player::{Loadout, Player};
use bevy::prelude::*;
use gd_formats::vfs::Database;

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
            // binds / potion / evade requests must land before casting reads them (UsePotion in CastingSet,
            // EvadeRequest before CombatSet); the game's input writers (grimdawn's HotSlotKey / CastRequest)
            // order before this chain so their requests are taken the same frame.
            .add_systems(
                Update,
                (show_skill_set, hot_slot_keys, arming, use_options)
                    .chain()
                    .in_set(HotbarSet)
                    .after(crate::player::PlayerSet)
                    .before(crate::casting::CastingSet),
            );
    }
}

/// The hotbar's cursor readers ([`record_cursor`], [`new_mouse_command`]): before the game's input writers
/// that read the cursor record (grimdawn's cast chain) and the request chain ([`HotbarSet`]).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HotbarCursorSet;

/// The hotbar's request chain (`hot_slot_keys` → `arming` → `use_options`): after the frame's input writers
/// and before `CastingSet`, so a key press or a right-click option becomes its cast / potion / evade request
/// the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HotbarSet;

/// Another hotbar page shown, the HUD's request (`grimdawn` `ui::hud`: Show Alternate Hot Slot Set, Quickbar Switch
/// and the row button): [`Caster::next_displayed_set`] / [`Caster::reset_displayed_set`] on the player's hotbar.
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

/// The Move skill (`Skill_Move`, `SkillManager::GetDefaultSkillId(3)`: the default skills are `SkillManager`
/// +416.. in the order weapon attack 0, basic pool attack 1, kick 2, move 3, pet attack 4, health 5, energy 6,
/// evade 7, `LoadDefaultPlayerSkills` @0x18052e3f0). Its request always fails
/// (`Skill_Move::GetValidTarget` returns false, vtable +1200), so on a mouse slot it only moves:
/// [`move_slot_keeps_target`].
pub const MOVE_TO: &str = "records/skills/default/defaultmoveto.dbr";

/// Whether `skill` is the Move skill ([`MOVE_TO`]).
pub fn is_move_to(skill: &str) -> bool {
    skill.eq_ignore_ascii_case(MOVE_TO)
}

/// What a world press found under the cursor, for [`move_slot_keeps_target`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressedObject {
    /// A `Player` or a `Monster` (foes, pets).
    Character,
    /// A `Destructible` (a breakable).
    Destructible,
    /// Anything else the pick gives (an NPC, a fixed actor...).
    Other,
}

/// A world mouse press (or hold) whose slot option is the Move skill (the armed slot's, else the left button's
/// primary slot / the right button's secondary slot; `Grim Dawn.exe` sub_140021000 @0x1400224e9): the
/// object under the cursor stays the command's target only for a
/// left (primary) press on something that is not a character, and on a breakable only with the option
/// `breakingMoveTo` ("\"Move To\" Breaks Objects", bool 33, default on). Otherwise the press is a plain move to the
/// cursor point (the object's coordinates when there is one): the target id is dropped, the button acts as the
/// primary one (`a1+133 = 1`, so a right press with Move walks too) and the command counts as repeated
/// (`SetCommandRepeated`). A kept target goes to `SelectPrimaryAction` @0x180161d00, whose Move request fails, so the
/// default weapon attack (`GetDefaultSkillId(0)`) hits it: Move breaks breakables (`combat::player_attack`, the
/// left Move skill not being a [`crate::casting::SpellAttacker`]) but never attacks a monster.
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

/// [`move_slot_keeps_target`] for a picked character / breakable — the classification every caller shared:
/// a breakable is a [`PressedObject::Destructible`], anything else a [`PressedObject::Character`].
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

/// Whether the primary (left-button) action on this `Destructible` record is
/// the kick, whatever skill the left slot holds:
/// `ControllerPlayerState::SelectPrimaryAction` @0x180161d00 requests
/// `GetDefaultSkillId(2)` ([`crate::casting::DEFAULT_KICK`]) for a
/// `Destructible` whose dword +546 is 47, before and instead of the slot's
/// skill; `Destructible::Load` @0x180261530 sets +546 to 47 exactly when
/// `attackAnimation` is "Kick" (17 otherwise, "Attack" and "Attack;Kick"
/// included). The right button (`SelectSecondaryAction` @0x180162290) and
/// hot-slot keys have no such branch. The kick itself is a melee attack
/// (`Skill_Kick::GetValidTarget` @0x1804f9020 → `Skill::GetValidMeleeTarget`
/// @0x180497460) with the record's `weaponDamagePct` 150 and flat physical
/// 8–10 (`Skill_Kick::TargetResult` @0x1804f8cd0).
pub fn kicks_breakable(rec: &gd_formats::arz::Record) -> bool {
    rec.str("attackAnimation").is_some_and(|a| a == "Kick")
}

/// The default player skills of the UI skill list, in its order (the basic
/// pool attack and the kick are never listed: see the module docs).
pub const DEFAULT_SKILLS: [&str; 6] = [
    MOVE_TO,
    crate::casting::DEFAULT_ATTACK,
    "records/skills/default/defaultpetattack.dbr",
    HEALTH_POTION,
    ENERGY_POTION,
    EVADE,
];

/// The armed hot slot (`PlayerHotSlotCtrl` `SkillSet` +0, `IsAnySlotActive`):
/// clicked with the mouse, waiting for its target (module docs). Set and
/// cleared by [`arming`] only; the UI reads it.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArmedSlot(pub Option<usize>);

/// The controller's cursor record (module docs): the character that was
/// under the cursor and the world point, at the last mouse event over the
/// world (in Keyboard Only the controller targeting's pick instead, `crate::controller_target`). Set by
/// [`record_cursor`] only; `point` None until the first one (and for an empty controller record).
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct WorldCursor {
    pub target: Option<Entity>,
    pub point: Option<Vec3>,
}

/// A mouse event over the world (the UI did not take it): the character under
/// the cursor (a foe or one's own pet) and its coordinates, else the ground
/// point under the cursor (`SetMouseRepeatData`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct CursorOverWorld {
    pub target: Option<Entity>,
    pub point: Vec3,
}

/// [`CursorOverWorld`] (the mouse) or, in Keyboard Only, [`ControllerTarget`](crate::controller_target::ControllerTarget)
/// (the controller targeting, which may empty the record) → [`WorldCursor`] (the last event of the frame wins).
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

/// A mouse event that reached the world (the UI did not take it): a left / right press, and whether a left or
/// right button is held. The game sends one per press and one per frame (the input device's move event).
/// `object`: the press is a primary action on an item, an NPC or a fixed actor (`SelectPrimaryAction`'s requests,
/// module docs).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct WorldMouse {
    pub press: bool,
    pub held: bool,
    pub object: bool,
}

/// `ControllerPlayer::IsNewMouseCommand` (+1073, module docs): a world press is pending, so held hot slot keys do
/// not repeat. Set by [`new_mouse_command`] and [`hot_slot_keys`] only.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct NewMouseCommand(pub bool);

/// [`WorldMouse`] → [`NewMouseCommand`]: a press sets it, its primary action on an item / NPC / fixed actor clears
/// it again (`SelectPrimaryAction`), an event with no button held clears it (sub_140021000).
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

/// A hot slot key went down or up (the UI sends both; `slot` is the caster
/// slot the press used).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct HotSlotKey {
    pub slot: usize,
    pub down: bool,
}

/// The held hot slot keys (HUD +29296, module docs): repeatable slots whose
/// key is down, the most recent first, and the time of the last press (the
/// 50 ms lock). Set by [`hot_slot_keys`] only.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct HeldHotSlots {
    pub slots: std::collections::VecDeque<usize>,
    pub last_press: Option<f32>,
}

/// The lock after a hot slot key press or repeat (sub_14020D550: 50 ms).
pub const REPEAT_LOCK: f32 = 0.05;

/// Hot slot keys (module docs): a press requests the slot's option at the
/// cursor record and holds a repeatable slot; the update repeats the front
/// held slot once the lock is over and the player stands.
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
            Has<crate::combat::Dead>,
        ),
        With<Player>,
    >,
    alive: Query<(), Without<crate::combat::Dead>>,
    options: Option<Res<crate::options::GameOptions>>,
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
    // An empty record (no region): in the keyboard movement modes the player's own coordinates
    // (`HotSlotOptionSkill::Activate` @0x18031bbc0). PORT: in Classic a point 5 m
    // ahead (the engine's request with an empty point was not traced).
    let keyboard = options.as_ref().is_some_and(|o| o.int("movementType") > 0);
    let empty = if keyboard {
        tf.translation
    } else {
        tf.translation + tf.rotation * Vec3::Z * 5.0
    };
    // `RequestSkillAction` at the controller's cursor record (+1084 / +1088).
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
            // The last held key let go: a new mouse command (sub_140210EB0).
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

/// A world press with a slot armed: use it at `target` (the character under
/// the cursor: a foe or one's own pet) or the ground `point`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct UseArmedSlot {
    pub target: Option<Entity>,
    pub point: Vec3,
}

/// The armed slot's rules (module docs): every [`CastRequest`] (a hot slot
/// key, a slot used at once) drops it, [`DisarmHotSlot`] drops it, an
/// [`ActivateHotSlot`] uses its option at once or arms it, [`UseArmedSlot`] /
/// [`CastOnAlly`] take it and request its skill.
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
    pets: Query<(&Transform, &crate::pets::Pet), (Without<Player>, Without<crate::combat::Dead>)>,
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
            // At the player (`HotSlotOptionSkill::Activate` passes the
            // player's coordinates); an evade option aims by the cursor
            // record itself (`crate::evade::start_evade`).
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
        // The portrait: the pet as the combat ally, its coordinates
        // (sub_140254700 → `HandleActionFromMouse`); only one's own living pet.
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

/// What a slot's record is.
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

/// Bind `option` (None = empty) to the shown hotbar page's `slot` (the game
/// sends it for a pick of the slot list).
#[derive(Message, Clone, Debug)]
pub struct BindSlot {
    pub slot: usize,
    pub option: Option<String>,
}

/// The pick list of `slot` (module docs), `None` = the empty pick; `effective` the player's
/// [`crate::player::EffectiveSkills`].
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
        .filter(|n| !crate::player::is_default_skill(n));
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
            .map(|p| p.item.base_name.clone())
            .filter(|n| db.get(n).is_some_and(|r| r.class == "OneShot_Scroll"))
            .collect();
        scrolls.sort();
        scrolls.dedup();
        out.extend(scrolls.into_iter().map(Some));
    }
    out.push(None);
    out
}

/// Binds the requested options; an option that is neither a fixed option nor a
/// database record (a stale or bad UI request) is refused.
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

/// [`BindSlot`]s; a used slot holding a potion, evade or scroll option does
/// what the R / E keys, Space or a right click on the item in the bags do
/// (`casting.rs` casts only the skill options).
#[allow(clippy::too_many_arguments)]
fn use_options(
    mut binds: MessageReader<BindSlot>,
    mut requests: MessageReader<CastRequest>,
    data: Res<GameData>,
    mut players: Query<&mut Caster, With<Player>>,
    // Option: binds and potion / evade options work without an inventory; only the scroll option needs it.
    inv: Option<ResMut<Inventory>>,
    mut potions: MessageWriter<UsePotion>,
    mut evades: MessageWriter<EvadeRequest>,
) {
    bind_slots(&data.db, &mut binds, &mut players);
    let mut inv = inv;
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
                // A held key / button repeats the request: not while the
                // scroll cools down (the use would be refused anyway).
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
