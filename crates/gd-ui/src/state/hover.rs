//! The hover target the world mouse handler picks (`hover.rs`) and the cursor mark it carries:
//! `gd_ui::cursor` reads them, the binary's hover system writes them.
use bevy::prelude::*;
use std::sync::Arc;

/// What the hover target is (decides the widget's parts).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Kind {
    /// A fixed item: name only.
    #[default]
    Object,
    /// An `Npc`: name only.
    Npc,
    /// Another character: name, race, health bar (when it has a life),
    /// tier border, level.
    Character,
    /// A `Destructible`: name, health bar, tier-1 border.
    Breakable,
}

/// What the world mouse handler (Grim Dawn.exe `InGameUI::HandleMouseEvent`) marks for the
/// cursor about the hover target (`ui::cursor` reads it).
/// `InGameUI::HandleMouseEvent`: `Attack` (`InGameUI.hoverAttack`) when the object under the
/// cursor has an id and the controller's combat enemy is set (an enemy, a
/// targetable breakable); for a visible `Npc` that `IsOfInterest` (a plain
/// `Npc`: `Npc::IsOfInterest`; every `Npc` subclass's slot returns true),
/// `Merchant` (`InGameUI.hoverMerchant`) when its class is `NpcMerchant` (or derives from it:
/// no stock class does), else `Dialog` (`InGameUI.hoverDialog`); then the actor's record
/// `interactIconOverride` (`Actor::Load`, `GetInteractIconOverride`):
/// "Dialog" clears the flags and sets `Dialog`, "Attack" sets `Attack`.
/// The combat enemy is not the hover target: `InGameUI::HandleMouseEvent` clears
/// it each event, picks the object under the cursor from the whole list
/// (`InGameUIActorCapture::PickObjectUnderCursor` over the frame's candidates) and, when there is one, sets
/// the combat enemy from the enemy list's pick (the touched enemy /
/// targetable breakable nearest to the player); so a container, door or
/// non-interest NPC that wins the hover over an enemy still gets `Attack`,
/// while an NPC's merchant / dialog marks outrank it in `ui::cursor::select`
/// (the binary's `hover::marks`). While an enemy-target skill runs (`IsRunningSkill`, the running skill's
/// `GetTargetType` 2), an ally under the cursor drops the object: no hover target, no mark. With `targetLock`
/// the held button keeps the object (`hover::held_pick`).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Interact {
    #[default]
    None,
    Attack,
    Dialog,
    Merchant,
}

/// The pointer position the pick used this frame (logical px; the
/// `GD_HOVER` stand-in when set), `None` outside the window. Written by
/// the binary's `hover::pick` only.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
pub struct HoverPoint(pub Option<Vec2>);

/// The hover target and what the widget shows of it.
#[derive(Resource, Default, PartialEq, Clone, Debug)]
pub struct Hovered {
    pub entity: Option<Entity>,
    pub name: Arc<str>,
    pub kind: Kind,
    /// The cursor's mark for it.
    pub interact: Interact,
    /// The outline colour (`None`: not outlined).
    pub outline: Option<Vec3>,
    /// Race line (characters).
    pub race: Arc<str>,
    /// `monsterClassification` index (Common 0 … SuperBoss 5).
    pub classification: u8,
}
