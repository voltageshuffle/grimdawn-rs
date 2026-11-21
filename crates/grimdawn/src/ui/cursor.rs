//! The game's mouse cursor, drawn as a software sprite over everything.
//!
//! The set (Grim Dawn.exe sub_14029E1E0, the cursor manager's
//! loader; `GraphicsEngine::LoadCursor` each, kind index → slot +16 + 8 i,
//! the large ones at +120 + 8 i): 1 `ui/cursor/cursordefault.tex`,
//! 2 / 5 `cursorreticle.tex`, 3 / 6 `cursorreticleattack.tex`,
//! 4 `cursorattack.tex`, 7 `cursoropen.tex`, 8 `cursordialog.tex`,
//! 9 `cursormerchant.tex`, 10 `cursormerchantnobuy.tex`,
//! 11 `cursornoplacement.tex`, 12 `cursortextedit.tex`; each has an `_lg`
//! twin. sub_14029EFE0 sets kind i (only when it changes) through
//! `RenderDevice::SetHardwareCursor(i, 0, 0, data)`, i + 13 with the large
//! set, picked by `largeCursor` (option bool 76, checked each update by
//! sub_14029EEB0).
//!
//! The look (Direct3D11.dll `Direct3DDevice11::SetHardwareCursor`):
//! a Windows hardware cursor (`CreateIconIndirect`) from the texture's top
//! level, 32-bit BGRA only (the stock files: 32 × 32, `_lg` 64 × 64), at its
//! own pixel size (not UI-scaled), hotspot (0, 0) for every kind (the
//! reticles too, so their centre sits 16 px right of and below the point
//! the click uses).
//!
//! Which one (sub_14002D920, every game-window update after the
//! frame's picking), first match:
//! 1. pets selected for an order (game window +2152, the selection the
//!    next world click orders): 6 reticle-attack over an attack target, else
//!    5 reticle;
//! 2. +2105, a merchant NPC under the cursor: 9 merchant;
//! 3. +2107, a vendor item under the cursor the player cannot afford
//!    (sub_1401FC000: `GetMarketItemStatus` 1 and not
//!    `GetMarketIsItemAffordable`): 10 merchant-no-buy;
//! 4. +2104, another NPC of interest (or an `interactIconOverride`
//!    "Dialog"): 8 dialog;
//! 5. +2108, a held item over a slot that refuses it: 11 no-placement. Set by
//!    sub_1401AE6F0 from the slot widget's mouse handler: the item ascension
//!    slot (sub_1401B15C0: not `ItemEquipment`, or a replica field set) and
//!    the crafter's blueprint slot (sub_1401AED10: not an
//!    `ItemArtifactFormula`). That slot never takes the mouse: it is the
//!    crafting window's member +1392 (ctor sub_140196390, slot ctor
//!    sub_1401AD920: rect 0 × 0), the window's loader sub_14019C5A0 does not
//!    read the record's `artifactFormulaBox` (no such string in the exe) and
//!    nothing else sets the rect, so its hit test (sub_1401AE6F0, a half-open
//!    rect) never passes: by design the port has no such slot;
//! 6. a hot slot armed (`IsAnyActiveTargetingNeeded`: `ActivateHotSlot`
//!    keeps a slot armed only when its option asks for a target): 3
//!    reticle-attack over an attack target (+2106), else 2 reticle;
//! 7. 4 attack over an attack target, else 1 default.
//!
//! The marks come from the world mouse handler sub_140021000 (`hover::
//! Interact`): every mouse event that reaches it clears +2104…+2108 before
//! the HUD takes it, so over a window only the HUD's own marks (3, 5) count.
//! A held item keeps the cursor (`CursorHandler::GetGameCursorType` is 1;
//! the Enchant / RelicCombine / Transmute handlers' 7 and the text edit's 12
//! are set from other paths that the next sub_14002D920 overwrites, so they
//! never show in game); the main menu sets 1 (sub_1400105E0).
//!
//! PORT: the OS hardware cursor is a [`CustomCursor`] from the kind's stock texture (hotspot (0,0)),
//! set when the kind changes; a UI sprite stands in only for the `GD_HOVER` hook (the OS pointer
//! hides then, the sprite's node moves with [`HoverPoint`]). The no-placement mark covers the item
//! ascension slot (`Trade::accepts`; the blueprint slot never shows it, see
//! above); no gamepad, so the cursor never hides for input mode 1. The attack
//! mark's held-button (`targetLock`) and running-skill cases are `hover`'s
//! ([`crate::hover::Interact`]).
use crate::hover::{HoverPoint, Hovered, Interact};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::window::{CursorIcon, CursorOptions, CustomCursor, CustomCursorImage, PrimaryWindow};
use gd_formats::save::item::Item;

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameCursor>().add_systems(
            PostUpdate,
            (choose, (os_pointer, draw))
                .chain()
                .before(bevy::window::CursorSystems::Update)
                .before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// The cursor kinds, by the engine's index (module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorKind {
    #[default]
    Default = 1,
    Reticle = 2,
    ReticleAttack = 3,
    Attack = 4,
    PetReticle = 5,
    PetReticleAttack = 6,
    Dialog = 8,
    Merchant = 9,
    MerchantNoBuy = 10,
    NoPlacement = 11,
}

impl CursorKind {
    /// The texture (`large`: the `_lg` twin, option `largeCursor`).
    pub fn texture(self, large: bool) -> String {
        let name = match self {
            Self::Default => "cursordefault",
            Self::Reticle | Self::PetReticle => "cursorreticle",
            Self::ReticleAttack | Self::PetReticleAttack => "cursorreticleattack",
            Self::Attack => "cursorattack",
            Self::Dialog => "cursordialog",
            Self::Merchant => "cursormerchant",
            Self::MerchantNoBuy => "cursormerchantnobuy",
            Self::NoPlacement => "cursornoplacement",
        };
        format!("ui/cursor/{name}{}.tex", if large { "_lg" } else { "" })
    }
}

/// The marks the choice reads (the game window's flags, module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CursorMarks {
    /// Pets selected for an order (+2152).
    pub pet_order: bool,
    /// An attack target under the cursor (+2106).
    pub attack: bool,
    /// A merchant under the cursor (+2105).
    pub merchant: bool,
    /// A vendor item under the cursor the player cannot afford (+2107).
    pub cannot_afford: bool,
    /// An NPC to talk to under the cursor (+2104).
    pub dialog: bool,
    /// A held item over a slot that refuses it (+2108).
    pub no_placement: bool,
    /// A hot slot armed, waiting for its target.
    pub targeting: bool,
}

/// sub_14002D920 (module docs).
pub fn select(m: &CursorMarks) -> CursorKind {
    use CursorKind::*;
    if m.pet_order {
        return if m.attack {
            PetReticleAttack
        } else {
            PetReticle
        };
    }
    if m.merchant {
        Merchant
    } else if m.cannot_afford {
        MerchantNoBuy
    } else if m.dialog {
        Dialog
    } else if m.no_placement {
        NoPlacement
    } else if m.targeting {
        if m.attack { ReticleAttack } else { Reticle }
    } else if m.attack {
        Attack
    } else {
        Default
    }
}

/// The cursor shown and its texture set. Written by [`choose`] only.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct GameCursor {
    pub kind: CursorKind,
    pub large: bool,
}

#[derive(Component)]
struct CursorSprite;

/// The marks from the hover target, the armed slot, the pet selection and
/// the trade window, then [`select`].
#[allow(clippy::too_many_arguments)]
fn choose(
    hovered: Res<Hovered>,
    point: Res<HoverPoint>,
    window: Query<&Window, With<PrimaryWindow>>,
    armed: Res<gd_game::hotbar::ArmedSlot>,
    pets: Res<crate::pets::PetSelection>,
    trade: Res<crate::trade::Trade>,
    trade_layout: Res<crate::trade::TradeLayout>,
    inv: Option<Res<crate::inventory::Inventory>>,
    hud: Res<super::HudValues>,
    options: Res<gd_game::options::GameOptions>,
    data: Res<crate::data::GameData>,
    pause: Res<gd_game::pause::GamePause>,
    mut refused: Local<Option<(Item, bool)>>,
    mut cursor: ResMut<GameCursor>,
) {
    let gold = hud.gold;
    let cannot_afford = trade
        .hover
        .as_ref()
        .and_then(|h| h.1)
        .is_some_and(|price| price > gold);
    // The ascension slot under a held item (`Trade::accepts`, cached per item).
    let scale = window.single().map_or(1.0, |w| w.scale_factor());
    let over_slot = Some(&*trade)
        .filter(|t| matches!(t.open, Some(crate::trade::TradeOpen::Ascension { .. })))
        .and(trade_layout.slot)
        .zip(point.0)
        .is_some_and(|(r, p)| crate::trade::contains(&r, p * scale));
    let no_placement = match (
        over_slot,
        inv.as_ref().and_then(|i| i.held.as_ref()),
        Some(&*trade),
    ) {
        (true, Some(h), Some(t)) => {
            if refused.as_ref().is_none_or(|(i, _)| i != h) {
                *refused = Some((h.clone(), t.accepts(h, &data).is_err()));
            }
            refused.as_ref().is_some_and(|r| r.1)
        }
        _ => false,
    };
    // The game menu / its confirm / Options take the pointer: no world or window marks under them.
    let marks = if pause.menu {
        CursorMarks::default()
    } else {
        CursorMarks {
            pet_order: !pets.0.is_empty(),
            attack: hovered.interact == Interact::Attack,
            merchant: hovered.interact == Interact::Merchant,
            cannot_afford,
            dialog: hovered.interact == Interact::Dialog,
            no_placement,
            targeting: armed.0.is_some(),
        }
    };
    let next = GameCursor {
        kind: select(&marks),
        large: options.flag("largeCursor"),
    };
    if *cursor != next {
        debug!(
            "cursor: {:?}{}",
            next.kind,
            if next.large { " (large)" } else { "" }
        );
        *cursor = next;
    }
}

/// The OS hardware cursor is the cursor (Direct3D11 `SetHardwareCursor` /
/// `CreateIconIndirect`, hotspot (0,0)): a [`CustomCursor`] from the kind's stock texture, set only
/// when the kind changes. Only the `GD_HOVER` stand-in hides it ([`draw`]'s sprite takes over).
#[allow(clippy::too_many_arguments)]
fn os_pointer(
    mut commands: Commands,
    mut windows: Query<(Entity, &mut CursorOptions), With<PrimaryWindow>>,
    hooks: Res<crate::hooks::Hooks>,
    cursor: Res<GameCursor>,
    data: Res<crate::data::GameData>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Local<Option<GameCursor>>,
    // One image per cursor texture: winit keeps an OS cursor per image handle.
    mut cache: Local<std::collections::HashMap<String, Option<Handle<Image>>>>,
    mut fresh: gd_game::session::NewSession,
) {
    // A new session may mount a mod with other cursor textures.
    if fresh.begun() {
        cache.clear();
        *shown = None;
    }
    let Ok((e, mut o)) = windows.single_mut() else {
        return;
    };
    let hide = hooks.ui.hover.is_some();
    if o.visible == hide {
        o.visible = !hide;
    }
    if *shown == Some(*cursor) {
        return;
    }
    *shown = Some(*cursor);
    let path = cursor.kind.texture(cursor.large);
    let h = cache
        .entry(path.clone())
        .or_insert_with(|| cursor_image(&data, &path).map(|i| images.add(i)))
        .clone();
    if let Some(h) = h {
        // Bevy 0.20: the cursor icon is a component on the window entity (not a CursorOptions field).
        commands
            .entity(e)
            .insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
                handle: h,
                hotspot: (0, 0),
                ..default()
            })));
    }
}

/// The cursor texture as winit takes it: RGBA8 kept on the CPU (the stock textures are BGRA / DXT, and the
/// shared loader's images are render-world only).
fn cursor_image(data: &crate::data::GameData, path: &str) -> Option<Image> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let bytes = data.vfs.read(&gd_formats::vfs::normalize(path)).ok()?;
    let tex = gd_formats::tex::Tex::parse_owned(bytes).ok()?;
    let f = tex.frames.first()?;
    let px = f.decode_rgba8(0).ok()?;
    Some(Image::new(
        Extent3d {
            width: f.width,
            height: f.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    ))
}

/// One sprite: its image follows [`GameCursor`], its node the pointer. `shown` is the cursor the sprite's image
/// was set for, so it is forgotten whenever a new sprite is spawned (after an Exit to Main Menu that despawned a
/// sprite spawned inside the session, i.e. after `exit::capture_baseline`).
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    cursor: Res<GameCursor>,
    point: Res<HoverPoint>,
    window: Query<(&Window, &CursorOptions), With<PrimaryWindow>>,
    mut sprite: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<CursorSprite>>,
    data: Res<crate::data::GameData>,
    mut tex: ResMut<gd_render::textures::Textures>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Local<Option<GameCursor>>,
) {
    let Ok((win, os)) = window.single() else {
        return;
    };
    let at = point.0.filter(|_| !os.visible);
    let Ok((mut node, mut image, mut vis)) = sprite.single_mut() else {
        commands.spawn((
            CursorSprite,
            super::hud::KeepsShown,
            ImageNode::default(),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            GlobalZIndex(i32::MAX),
            Visibility::Hidden,
        ));
        *shown = None;
        return;
    };
    if *shown != Some(*cursor) {
        let Some(h) = tex.get(&data, &cursor.kind.texture(cursor.large), true, &mut images) else {
            return;
        };
        // Its own pixel size, not UI-scaled (module docs).
        let px = images
            .get(&h)
            .map_or(Vec2::splat(if cursor.large { 64.0 } else { 32.0 }), |i| {
                i.size().as_vec2()
            })
            / win.scale_factor();
        image.image = h;
        node.width = Val::Px(px.x);
        node.height = Val::Px(px.y);
        *shown = Some(*cursor);
    }
    let want = if at.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    vis.set_if_neq(want);
    if let Some(p) = at {
        // Hotspot (0, 0).
        let (l, t) = (Val::Px(p.x), Val::Px(p.y));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
    }
}
