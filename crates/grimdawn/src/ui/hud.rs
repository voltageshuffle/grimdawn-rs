//! The in-game HUD (`UIPlayerHud`, loader `Grim Dawn.exe` 0x1402139D0, draw
//! 0x14020A9D0).
//!
//! `standardHUD` (`Options` bool 71, the classic bar HUD): sub_1402139D0: the loader reads the
//! bool and loads `hud_mastertable.dbr` (true) or `hud_orbmastertable.dbr` (false, the default) as the
//! HUD's table; both name the same fields, so every layout read goes through [`master`] and the same
//! widgets draw either. The tables differ in the records behind the fields (the classic bar HUD's
//! `records/ui/hud/*`, the orb's `records/ui/hud/orbhud/*`; the status icons' record, `hudPlayerStatusIcons`),
//! in `hudStatusArea*` / `hudRegionArea*` / `hudFacadeArea*`, and in the fields only one has: the orb's
//! `hudMenuFolderButton` / `hudMenuFolderBitmap` / `playerLevelUp{Attribute,Skill}Button` (classic HUD:
//! no menu folder, the level-up indicator is `playerPortraitLevelUpIndicator{Bitmap,Button}`) and the
//! classic's `hudHealthBarOverlayBitmap` / `hudManaBarOverlayBitmap` (shared record). sub_14020A9D0 draws
//! the teleport / weapon swap buttons before the status manager (classic) or after `hudSlotsText` (orb);
//! sub_1400CD780 (options apply) reloads the HUD when the bool changed. Table fields the two share
//! unchanged (the Crucible / Shattered Realm windows, the hot slot select window, the loot filter
//! window) are read from [`MASTER`].
//!
//! Two anchor windows: `hudToolbar` (T, the skill bar) and
//! `hudPortraitAndStatsSummary` (P, orbs and the top panel). P is placed
//! directly above T (`P.y = T.y − P.h`) whatever its own `windowDefaultY`.
//! Child positions are `round(record pos × scale)` added to their anchor.
use super::window::Rebuild;
use super::{HudValues, Ui};
use crate::data::GameData;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{BarGraph, Rect as URect, TextBox, Widget, WindowDef};

/// The orb table: the fields both tables share unchanged read from it.
pub const MASTER: &str = "records/ui/hud/hud_orbmastertable.dbr";
pub const CLASSIC_MASTER: &str = "records/ui/hud/hud_mastertable.dbr";

/// The table of the chosen layout (`standardHUD`).
pub fn master(classic: bool) -> &'static str {
    if classic { CLASSIC_MASTER } else { MASTER }
}

/// `standardHUD`; the default (no options resource) is the orb HUD.
pub fn classic(options: Option<&gd_game::options::GameOptions>) -> bool {
    options.is_some_and(|o| o.flag("standardHUD"))
}
const AREA_NAME: &str = "records/ui/mapaerial/map_regionnamedisplaycompass.dbr";

/// Which value a bar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    Health,
    HealthGain,
    Constitution,
    Energy,
    EnergyReserved,
    Experience,
    /// The Shattered Realm's floor progress (`HudValues::endless_progress`).
    EndlessProgress,
    /// Always full (the endless HUD's orb: `SetValue(1, 0, 1)`).
    Full,
}

impl Stat {
    fn ratio(self, v: &HudValues, vit: &super::HudVitals) -> f32 {
        let r = match self {
            Stat::Health => vit.health / vit.health_max.max(1.0),
            Stat::Energy => vit.energy / vit.energy_max.max(1.0),
            Stat::EnergyReserved => vit.energy_reserved / vit.energy_max.max(1.0),
            Stat::Experience => v.experience,
            Stat::EndlessProgress => v.endless_progress,
            Stat::Full => 1.0,
            // sub_14020D550: the bar gets (int)current and (int)limit.
            Stat::Constitution => {
                vit.constitution.trunc() / (vit.constitution_max.trunc()).max(1.0)
            }
            Stat::HealthGain => vit.health_gain,
        };
        r.clamp(0.0, 1.0)
    }
}

/// A bar's fill node; `animate_bars` updates its rect, source rect and cap.
#[derive(Component)]
pub struct BarFill {
    pub bar: BarGraph,
    pub stat: Stat,
    /// Widget rect on screen (physical px).
    pub rect: URect,
    pub full_size: UVec2,
    pub cap: Option<(Entity, UVec2)>,
    pub inv: f32,
    pub scale: f32,
}

/// The button fields of the HUD table with a hit rect at the toolbar origin ([`HudLayout`]): the rollover's buttons,
/// the classic HUD's menu buttons, the row select button and the orb HUD's menu folder button.
const LAYOUT_BUTTONS: [&str; 15] = [
    "hudMenuOptionsButton",
    "hudMiniMapButton",
    "hudAchievementsButton",
    "hudFactionButton",
    "hudCharacterButton",
    "hudSkillsButton",
    "hudPartyButton",
    "hudQuestButton",
    "hudTeleportButton",
    "hudActiveTeleportButton",
    "hudButtonWeaponSwap",
    "hudCompassButton",
    "hudLootFilterButton",
    "hotSlotSelectUpButton",
    super::menufolder::BUTTON,
];

/// The orb HUD's level-up buttons ('+': attribute, skill points to spend), at the portrait origin.
pub const LEVEL_UP_BUTTONS: [&str; 2] =
    ["playerLevelUpAttributeButton", "playerLevelUpSkillButton"];

/// The hot slot fields with a hit rect ([`HudLayout`]); `HOT_SLOTS[i]` is hot slot `i + 1`.
pub const HOT_SLOTS: [&str; 10] = [
    "hudHotSlot1",
    "hudHotSlot2",
    "hudHotSlot3",
    "hudHotSlot4",
    "hudHotSlot5",
    "hudHotSlot6",
    "hudHotSlot7",
    "hudHotSlot8",
    "hudHotSlot9",
    "hudHotSlot10",
];
const LAYOUT_SLOTS: [&str; 5] = [
    "hudEvadeButton",
    "hudPriPotionButton",
    "hudSecPotionButton",
    "hudPriSkillButton",
    "hudSecSkillButton",
];
const LAYOUT_BARS: [&str; 3] = ["hudManaBar", "hudHealthBar", "hudExperienceBar"];

/// The HUD's background hit areas ([`HudLayout`], sub_14020C140): the toolbar window
/// and the status / facade / region strips, so a click on the HUD art does not reach the world.
/// Keys of [`HudLayout`] only (not master-table fields).
pub const TOOLBAR_AREA: &str = "hudToolbarArea";
const STATUS_AREA: &str = "hudStatusArea";
const FACADE_AREA: &str = "hudFacadeArea";
const REGION_AREA: &str = "hudRegionArea";

/// The screen rects (physical px) of the HUD's static hit areas, by HUD table field: buttons
/// ([`button_rect_at`]; the orb HUD's folder buttons at the folder origin, `menufolder.rs`, with the folder's
/// background), hot slots ([`slot_rect`]), the orbs and the experience bar ([`bar_rect`]), the level-up indicator
/// ([`level_up_button_rect`]) and the orb HUD's level-up buttons ([`LEVEL_UP_BUTTONS`]). PORT (a performance
/// choice): [`build`] writes it, so the per-frame hit tests read a rect instead of re-deriving it from the database;
/// it changes with the HUD (window size, classic / orb table), and a field the table lacks has no rect.
#[derive(Resource, Default)]
pub struct HudLayout(bevy::platform::collections::HashMap<&'static str, URect>);

impl HudLayout {
    pub fn rect(&self, field: &str) -> Option<URect> {
        self.0.get(field).copied()
    }

    /// Whether physical-px point `p` is over any of the HUD's hit areas.
    pub fn covers(&self, p: Vec2) -> bool {
        self.0.values().any(|r| super::contains(r, p))
    }

    /// `folder`: the menu folder's shown parts; its buttons and background are hit areas only while drawn
    /// (a closed folder leaves no dead strip left of the toolbar).
    fn of(ui: &mut Ui, folder: super::menufolder::FolderView) -> Self {
        let mut rects = bevy::platform::collections::HashMap::default();
        for f in LAYOUT_BUTTONS {
            if let Some(r) = super::rollover::button_rect(ui, f) {
                rects.insert(f, r);
            }
        }
        for f in HOT_SLOTS.into_iter().chain(LAYOUT_SLOTS) {
            if let Some(r) = slot_rect(ui, f) {
                rects.insert(f, r);
            }
        }
        for f in LAYOUT_BARS {
            if let Some(r) = bar_rect(ui, f) {
                rects.insert(f, r);
            }
        }
        if let Some(r) = level_up_button_rect(ui) {
            rects.insert(LEVEL_UP_BUTTON, r);
        }
        if !ui.classic_hud {
            // The orb table's menu buttons sit in the folder, not on the toolbar.
            for f in super::menufolder::BUTTONS {
                rects.remove(f);
            }
            if let Some(o) = super::menufolder::origin(ui) {
                if folder.buttons {
                    for f in super::menufolder::BUTTONS {
                        if let Some(r) = button_rect_at(ui, f, o) {
                            rects.insert(f, r);
                        }
                    }
                }
                if folder.open
                    && let Some(r) = super::menufolder::background_rect(ui, o)
                {
                    rects.insert(super::menufolder::BACKGROUND, r);
                }
            }
            if let Some(po) = portrait_origin(ui) {
                for f in LEVEL_UP_BUTTONS {
                    if let Some(r) = button_rect_at(ui, f, po) {
                        rects.insert(f, r);
                    }
                }
            }
        }
        // The HUD's background hit areas (sub_14020C140): the toolbar window rect and
        // the status / facade / region strips at the portrait origin (sub_1402139D0), so a click
        // on the toolbar art, the gaps between slots or the status strip does not reach the world.
        if let Some(m) = ui.record(ui.hud_master())
            && let Some(t) = m.str("hudToolbar").and_then(|p| ui.record(p))
        {
            let r = WindowDef::from_record(&t).screen_rect(ui.screen.x, ui.screen.y, ui.s);
            rects.insert(TOOLBAR_AREA, r);
        }
        if let Some(po) = portrait_origin(ui) {
            for (name, x, y, w, h) in [
                (
                    STATUS_AREA,
                    "hudStatusAreaX",
                    "hudStatusAreaY",
                    "hudStatusAreaXSize",
                    "hudStatusAreaYSize",
                ),
                (
                    FACADE_AREA,
                    "hudFacadeAreaX",
                    "hudFacadeAreaY",
                    "hudFacadeAreaXSize",
                    "hudFacadeAreaYSize",
                ),
                (
                    REGION_AREA,
                    "hudRegionAreaX",
                    "hudRegionAreaY",
                    "hudRegionAreaXSize",
                    "hudRegionAreaYSize",
                ),
            ] {
                if let Some(r) = area_rect(ui, po, x, y, w, h) {
                    rects.insert(name, r);
                }
            }
        }
        Self(rects)
    }
}

/// A HUD area strip's screen rect at `origin` from the master table's `x`/`y`/`XSize`/`YSize`
/// fields (sub_1402139D0: the status / facade / region areas sit at the portrait
/// origin, scaled); none when the table lacks them or they are empty.
fn area_rect(ui: &Ui, origin: Vec2, x: &str, y: &str, w: &str, h: &str) -> Option<URect> {
    let m = ui.record(ui.hud_master())?;
    let f = |k: &str| m.f32(k).unwrap_or(0.0);
    let (wv, hv) = (f(w), f(h));
    if wv <= 0.0 || hv <= 0.0 {
        return None;
    }
    Some(URect::new(
        origin.x + ui.sc(f(x)),
        origin.y + ui.sc(f(y)),
        ui.sc(wv),
        ui.sc(hv),
    ))
}

/// `loot_button`: the loot filter button's state (`lootfilter.rs`); `menu_down`: the factions and achievements
/// windows are open (their classic HUD buttons show the down bitmap).
pub fn build(
    ui: &mut Ui,
    values: &HudValues,
    loot_button: super::lootfilter::HudButton,
    menu_down: [bool; 2],
) {
    let layout = HudLayout::of(ui, values.menu_folder);
    ui.commands.insert_resource(layout);
    let Some(m) = ui.record(ui.hud_master()) else {
        return;
    };
    let field = |k: &str| m.str(k).map(str::to_string).unwrap_or_default();
    let (sw, sh) = (ui.screen.x, ui.screen.y);

    let toolbar = ui
        .record(&field("hudToolbar"))
        .map(|r| WindowDef::from_record(&r))
        .unwrap_or_default();
    let t = toolbar.screen_rect(sw, sh, ui.s);
    let portrait = ui
        .record(&field("hudPortraitAndStatsSummary"))
        .map(|r| WindowDef::from_record(&r))
        .unwrap_or_default();
    let mut p = portrait.screen_rect(sw, sh, ui.s);
    p.y = t.y - p.h;
    let (to, po) = (Vec2::new(t.x, t.y), Vec2::new(p.x, p.y));
    // The hot slots' key labels are the bound keys' names (sub_1401E6C90), not literals.
    let label = |f: &str| values.hotbar_labels.get(f).map(String::as_str);

    // Draw order of UIPlayerHud's render function.
    ui.bitmap_field(&field("hudBaseBitmap"), to);
    minimap(
        ui,
        &values.area_tag,
        m.i32("floatingAreaNameY").unwrap_or(10) as f32,
    );
    bar_field(ui, &field("hudManaBar"), po, Stat::Energy);
    bar_field(ui, &field("hudManaReserveBar"), po, Stat::EnergyReserved);
    bar_field(ui, &field("hudHealthGainBar"), po, Stat::HealthGain);
    bar_field(ui, &field("hudHealthBar"), po, Stat::Health);
    bar_field(ui, &field("hudConstitutionBar"), po, Stat::Constitution);
    ui.bitmap_field(&field("hudHealthBarOverlayBitmap"), po);
    ui.bitmap_field(&field("hudManaBarOverlayBitmap"), po);
    bar_field(ui, &field("hudExperienceBar"), to, Stat::Experience);
    ui.bitmap_field(&field("hudTopBitmap"), po);
    if let Some(Widget::Text(tb)) = ui.widget(&field("hudCharLevelBox")) {
        level_box(ui, &tb, po, values.level);
    }
    // The classic HUD's level-up indicator (sub_14020A9D0: the bitmap, then its hidden button, over the
    // portrait; the orb HUD has the attribute / skill buttons below instead): only drawn additively in the
    // level-up pulse's colour while the indicator flag is set ([`pulse_hud`]), so the build spawns it hidden.
    if let Some(Widget::Bitmap { bitmap, x, y }) =
        ui.widget(&field("playerPortraitLevelUpIndicatorBitmap"))
    {
        pulse_glow(ui, &bitmap, x, y, po, Glow::Indicator);
    }
    // Level-up buttons (attribute / skill points to spend). sub_14020A9D0: over the plain button, the
    // same bitmap again in the level-up pulse's colour while the character / skills button pulse is on.
    if values.attribute_points > 0
        && let Some(Widget::Button {
            up: Some(up), x, y, ..
        }) = ui.widget(&field("playerLevelUpAttributeButton"))
    {
        state_button(
            ui,
            "playerLevelUpAttributeButton",
            &field("playerLevelUpAttributeButton"),
            po,
            true,
            false,
        );
        pulse_glow(ui, &up, x, y, po, Glow::AttributeButton);
    }
    if values.skill_points > 0
        && let Some(Widget::Button {
            up: Some(up), x, y, ..
        }) = ui.widget(&field("playerLevelUpSkillButton"))
    {
        state_button(
            ui,
            "playerLevelUpSkillButton",
            &field("playerLevelUpSkillButton"),
            po,
            true,
            false,
        );
        pulse_glow(ui, &up, x, y, po, Glow::SkillButton);
    }
    // Potion slots (R / E): the health / energy potion skills; evade (Space)
    // on the round centre button.
    let (pri, sec, ev) = (
        field("hudPriPotionButton"),
        field("hudSecPotionButton"),
        field("hudEvadeButton"),
    );
    let (hp, ep, evade) = (
        slot_icon(ui, &pri, crate::casting::HEALTH_POTION, &values.skills),
        slot_icon(ui, &sec, crate::casting::ENERGY_POTION, &values.skills),
        slot_icon(ui, &ev, crate::evade::EVADE, &values.skills),
    );
    hot_slot(ui, &pri, to, label("hudPriPotionButton"), hp.as_deref());
    hot_slot(ui, &sec, to, label("hudSecPotionButton"), ep.as_deref());
    hot_slot(ui, &ev, to, label("hudEvadeButton"), evade.as_deref());
    // The orb HUD's menu folder button and, while open, its background (`menufolder.rs`).
    if !ui.classic_hud {
        super::menufolder::draw_folder(ui, values.menu_folder, to);
        // The folder button's sounds + modal blocking (its bitmap is `menufolder.rs`'s).
        if let Some(r) = button_rect_at(ui, super::menufolder::BUTTON, to)
            && let Some(brec) = ui.record(&field(super::menufolder::BUTTON))
        {
            ui.record_button_sound(r, &brec);
        }
    }
    for (i, f) in HOT_SLOTS.into_iter().enumerate() {
        let icon = values
            .hotbar
            .get(i + values.hotbar_row * crate::casting::SLOT_ROW2)
            .cloned()
            .flatten();
        hot_slot(ui, &field(f), to, label(f), icon.as_deref());
    }
    let icon = |n: usize| values.hotbar.get(n).cloned().flatten();
    hot_slot(
        ui,
        &field("hudPriSkillButton"),
        to,
        label("hudPriSkillButton"),
        icon(crate::casting::SLOT_LEFT).as_deref(),
    );
    hot_slot(
        ui,
        &field("hudSecSkillButton"),
        to,
        label("hudSecSkillButton"),
        icon(crate::casting::SLOT_RIGHT).as_deref(),
    );
    ui.bitmap_field(&field("hudOverlayBitmap"), to);
    // Hotbar row selector (`hotSlotSelectUpButton`): the row number on the button.
    state_button(
        ui,
        "hotSlotSelectUpButton",
        &field("hotSlotSelectUpButton"),
        to,
        true,
        false,
    );
    // The row number (`hud_hotslotbarnumber.dbr`, a TextNumber in the HUD's `Localize("IntFormat", row % 2 + 1)`,
    // sub_14020D550 / sub_1402139D0, drawn right after the select button by sub_14020A9D0). The record's
    // `additiveBitmapName` glow sits behind the number, additive, around the laid-out text padded by
    // `additiveBitmapX/YPadding` (`GraphicsCanvas::RenderStyledRect` "additive" as the devotion art;
    // PORT: the padding-around-the-text geometry is the main menu name texture's, the TextNumber draw itself is not
    // decompiled).
    if let Some(tb) = ui
        .record("records/ui/hud/hud_hotslotbarnumber.dbr")
        .map(|r| gd_formats::ui::TextBox::from_record(&r))
    {
        let num = format!("{}", values.hotbar_row % 2 + 1);
        if let (Some((path, px, py)), Some(style)) = (tb.glow.clone(), tb.style.clone())
            && let Some((img, _)) = ui.image(&path)
        {
            let w = ui.text_width(&style, &num);
            let h = ui.line_height(&style);
            let (bx, by, bw, bh) = (
                to.x + ui.sc(tb.rect.x),
                to.y + ui.sc(tb.rect.y),
                ui.sc(tb.rect.w),
                ui.sc(tb.rect.h),
            );
            let tx = match tb.align_x {
                gd_formats::ui::TextAlign::Start => bx,
                gd_formats::ui::TextAlign::End => bx + bw - w,
                gd_formats::ui::TextAlign::Center => bx + (bw - w) * 0.5,
            };
            let ty = match tb.align_y {
                gd_formats::ui::TextAlign::Start => by,
                gd_formats::ui::TextAlign::End => by + bh - h,
                gd_formats::ui::TextAlign::Center => by + (bh - h) * 0.5,
            };
            let (px, py) = (ui.sc(px), ui.sc(py));
            super::additive::quad(
                ui,
                img,
                URect::new(
                    (tx - px).round(),
                    (ty - py).round(),
                    (w + 2.0 * px).round(),
                    (h + 2.0 * py).round(),
                ),
                Color::WHITE,
            );
        }
        ui.text(&tb, to, &num);
    }
    // The teleport button (`gd_game::riftgates` module doc): plain without a
    // personal portal, orange with one, the disabled bitmap while it waits.
    if let Some(t) = values.teleport {
        state_button(
            ui,
            teleport_field(t),
            &field(teleport_field(t)),
            to,
            !t.disabled,
            false,
        );
    }
    // Disabled until weapon set II was chosen once (`gd_game::weapon_sets`).
    state_button(
        ui,
        "hudButtonWeaponSwap",
        &field("hudButtonWeaponSwap"),
        to,
        values.weapon_swap,
        false,
    );
    state_button(
        ui,
        "hudCompassButton",
        &field("hudCompassButton"),
        to,
        true,
        false,
    );
    // The loot filter toggle (`lootfilter.rs`): down while the window is
    // open, disabled while all items are hidden.
    use super::lootfilter::HudButton;
    state_button(
        ui,
        "hudLootFilterButton",
        &field("hudLootFilterButton"),
        to,
        loot_button != HudButton::Disabled,
        loot_button == HudButton::Down,
    );
    // The menu buttons: the classic HUD's always on the toolbar, sub_14020A9D0 draws them at the origin they
    // were loaded at (the toolbar's, sub_1402139D0); the orb HUD's inside its opened folder, at the folder's origin
    // (`menufolder.rs`). Both draw the options button first, only while the options window is not up, then the others
    // only while the game menu is not up either ([`OptionsButton`]).
    if values.options_button.shown {
        state_button(
            ui,
            "hudMenuOptionsButton",
            &field("hudMenuOptionsButton"),
            to,
            true,
            values.options_button.down,
        );
    }
    if ui.classic_hud {
        if !values.options_button.down {
            menu_button_bitmaps(ui, &MENU_BUTTONS[1..], to, menu_down);
        }
    } else if values.menu_folder.buttons
        && let Some(o) = super::menufolder::origin(ui)
    {
        menu_button_bitmaps(ui, &super::menufolder::BUTTONS, o, menu_down);
    }
    mode_label(ui, &field("hudGameModeText"), &values.mode_tags);
}

/// The HUD's options button (`hudMenuOptionsButton`, both tables). `Grim Dawn.exe`: the draw sub_14020A9D0
/// draws it only while the options window is not up; the HUD's window listener sub_1402132A0 sets its down flag (+642)
/// when the game menu or the options window shows and clears it when they hide, so it shows the down bitmap while the
/// game menu is up (hover / press otherwise, as every HUD button: [`button_states`]). The orb toolbar art has the
/// button's socket with a plain grey gear; the button is drawn over it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptionsButton {
    pub shown: bool,
    pub down: bool,
}

/// Draws the menu buttons `fields` at `origin`. sub_14020A9D0: a pulsing button (character, skills; the
/// journal's and group's pulses never start in the exe) is drawn only additively in its pulse's colour, instead of its
/// plain bitmap ([`pulse_hud`]).
fn menu_button_bitmaps(ui: &mut Ui, fields: &[&'static str], origin: Vec2, menu_down: [bool; 2]) {
    let Some(m) = ui.record(ui.hud_master()) else {
        return;
    };
    let field = |k: &str| m.str(k).map(str::to_string).unwrap_or_default();
    for &f in fields {
        let rec = field(f);
        // sub_14020C140: the group button takes mouse events only with networking (never here), so it is
        // drawn in its plain up bitmap and neither hovers, presses, sounds nor shows a rollover (nothing sets its
        // disabled flag, so its `bitmapNameDisabled` never shows).
        if f == "hudPartyButton" {
            if let Some(Widget::Button {
                up: Some(up), x, y, ..
            }) = ui.widget(&rec)
            {
                ui.bitmap(&up, x, y, origin);
            }
            continue;
        }
        // sub_1402132A0 / sub_1402129E0: the faction button (+45170 down flag) is down while the
        // factions window is visible, the achievements button (+43514) while the achievements window is.
        let toggled = (f == "hudFactionButton" && menu_down[0])
            || (f == "hudAchievementsButton" && menu_down[1]);
        let pulse = match f {
            "hudCharacterButton" => Some(Glow::CharacterMenu),
            "hudSkillsButton" => Some(Glow::SkillsMenu),
            _ => None,
        };
        if let Some(g) = pulse {
            if let Some(Widget::Button {
                up: Some(up), x, y, ..
            }) = ui.widget(&rec)
                && let Some((e, _)) = state_button(ui, f, &rec, origin, true, false)
            {
                ui.commands.entity(e).insert(PlainOfGlow(g));
                pulse_glow(ui, &up, x, y, origin, g);
            }
        } else {
            state_button(ui, f, &rec, origin, true, toggled);
        }
    }
}

/// `GAME::ColorPulse` (Engine.dll), `LoadFromDatabase` @0x18004bcd0, `Update` @0x18004be80, `IsPulsing`
/// @0x18004bfe0, `Reset` @0x18004ba10, `SyncToColorPulse` @0x18004c000. A leg lasts `pulseTime` ms and blends A to B
/// (rising, the constructor's direction) or B to A; the end of a rising leg counts a cycle. At the start of the rising
/// leg after `numberOfCycles` (default -1: never) the leg blends A to the `defaultPulseColor*` instead (default
/// red -1: no such leg, the count steps once more and the A to B legs go on). `IsPulsing`: cycles 0 → false,
/// negative → true, else done cycles ≤ the count. The colour is kept between updates (zero before the first).
/// The start delay (+16) is never set for the HUD's pulses and is not modelled.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorPulse {
    pulse: f32,
    left: f32,
    rising: bool,
    cycles: i32,
    max_cycles: i32,
    a: [f32; 4],
    b: [f32; 4],
    default: [f32; 4],
    color: [f32; 4],
}

impl ColorPulse {
    pub fn from_record(r: &gd_formats::arz::Record) -> Self {
        let num = |k: &str, d: f32| r.f32(k).or_else(|| r.i32(k).map(|v| v as f32)).unwrap_or(d);
        let rgba =
            |p: &str, d: f32| ["Red", "Green", "Blue", "Alpha"].map(|c| num(&format!("{p}{c}"), d));
        let pulse = num("pulseTime", 0.0);
        Self {
            pulse,
            left: pulse,
            rising: true,
            cycles: 0,
            max_cycles: num("numberOfCycles", -1.0) as i32,
            a: rgba("pulseColorA", 0.0),
            b: rgba("pulseColorB", 0.0),
            default: rgba("defaultPulseColor", -1.0),
            color: [0.0; 4],
        }
    }

    /// Advances by `dt` ms and returns the colour.
    pub fn update(&mut self, dt: f32) -> [f32; 4] {
        self.left -= dt;
        let mut t = if self.pulse != 0.0 {
            (self.left / self.pulse).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.rising {
            t = 1.0 - t;
        }
        let lerp = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] {
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
        };
        if self.cycles != self.max_cycles || !self.rising {
            self.color = lerp(self.a, self.b);
        } else if self.default[0] >= 0.0 {
            self.color = lerp(self.a, self.default);
        } else {
            self.cycles += 1;
        }
        if self.left <= 0.0 {
            self.left = self.pulse;
            if self.rising {
                self.cycles += 1;
            }
            self.rising = !self.rising;
        }
        self.color
    }

    pub fn is_pulsing(&self) -> bool {
        self.max_cycles != 0 && (self.max_cycles < 0 || self.cycles <= self.max_cycles)
    }

    pub fn reset(&mut self) {
        self.left = self.pulse;
        self.cycles = 0;
    }

    /// Takes over `other`'s leg (time left, clamped to this pulse's length, and direction); one cycle less done.
    pub fn sync_to(&mut self, other: &ColorPulse) {
        self.left = if other.left >= 0.0 {
            other.left.min(self.pulse)
        } else {
            0.0
        };
        if self.cycles > 0 {
            self.cycles -= 1;
        }
        self.rising = other.rising;
    }

    pub fn color(&self) -> Color {
        let [r, g, b, a] = self.color;
        Color::srgba(r, g, b, a)
    }
}

/// A HUD element drawn additively in a pulse's colour ([`pulse_hud`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glow {
    /// The classic HUD's `playerPortraitLevelUpIndicatorBitmap` (indicator flag, level-up pulse).
    Indicator,
    /// The orb HUD's `playerLevelUpAttributeButton` (character flag, level-up pulse).
    AttributeButton,
    /// The orb HUD's `playerLevelUpSkillButton` (skills flag, level-up pulse).
    SkillButton,
    /// The classic `hudCharacterButton` (character flag, its own `hudWindowButtonColorPulse`).
    CharacterMenu,
    /// The classic `hudSkillsButton` (skills flag, its own `hudWindowButtonColorPulse`).
    SkillsMenu,
}

/// The additive copy of a pulsed element (hidden until [`pulse_hud`] shows it).
#[derive(Component)]
pub struct GlowNode {
    glow: Glow,
    image: Handle<Image>,
}

/// The plain bitmap a [`Glow`] replaces while it pulses (the classic menu buttons).
#[derive(Component)]
pub struct PlainOfGlow(Glow);

fn pulse_glow(ui: &mut Ui, bitmap: &str, x: f32, y: f32, origin: Vec2, glow: Glow) {
    let Some((image, size)) = ui.image(bitmap) else {
        return;
    };
    let r = URect::new(
        origin.x + ui.sc(x),
        origin.y + ui.sc(y),
        ui.sc(size.x as f32),
        ui.sc(size.y as f32),
    );
    let e = super::additive::quad(ui, image.clone(), r, Color::NONE);
    ui.commands
        .entity(e)
        .insert((Visibility::Hidden, GlowNode { glow, image }));
}

/// The HUD's pulse state (UIPlayerHud: the level-up pulse +1392, the window-button pulses +705136 (journal), +705168
/// (group), +705200 (character), +705232 (skills) with their flags at +24 of each; the indicator flag +705257).
#[derive(Default)]
pub struct HudPulses {
    level_up: Option<ColorPulse>,
    /// Character, skills.
    buttons: [Option<ColorPulse>; 2],
    on: [bool; 2],
    open: (bool, bool),
    /// The pulse records were looked up (found or not).
    loaded: bool,
}

/// The classic HUD's level-up indicator flag (UIPlayerHud +705257): set by the points-gained events, cleared when
/// the character or skills window opens ([`pulse_hud`]); while it is set the indicator pulses and a click on it
/// opens the skills window ([`menu_buttons`]).
#[derive(Resource, Default)]
pub struct LevelUpIndicator(pub bool);

/// Starts, advances and shows the HUD pulses (`Grim Dawn.exe`):
/// - sub_1402139D0 loads `playerPortraitLevelUpIndicatorColorPulse` and four `hudWindowButtonColorPulse` copies once
///   (kept over a `standardHUD` reload); sub_14020D550 advances the level-up pulse every frame (it never resets, so it
///   runs from the HUD's load and its `numberOfCycles` 10 only costs a frame: it keeps pulsing) and a button pulse while
///   its flag is on (off again once it stops pulsing: never, the record has no count).
/// - Game events (registered in sub_140205BE0; senders in Game.dll, listed in the `gd_game::progress` module doc): 48
///   (`Character::ReceiveExperience` after a level-up) starts the character pulse (sub_1402195F0); 49 (skill points
///   gained) the skills pulse and the indicator (sub_140219680); 50 (attribute points gained) the character pulse and
///   the indicator (sub_140219700). A pulse that starts is reset and then synced to the first running one (journal,
///   group, character, skills order; sub_14021BB10).
/// - Opening the character window (sub_140209BE0) clears the character pulse and the indicator; the skills window
///   (sub_14021A660) the skills pulse and the indicator.
/// - sub_14020A9D0 draws the glows with the "additive" style (`RenderStyledRect`) in the pulse's colour.
///
/// The events are [`gd_game::progress::LeveledUp`] (48) and [`gd_game::progress::PointsGained`] (49 / 50); a frame's
/// starts all reset their pulse, so their order does not change the phase.
#[allow(clippy::too_many_arguments)]
pub fn pulse_hud(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    panels: Res<super::UiPanels>,
    (mut leveled, mut gained): (
        MessageReader<gd_game::progress::LeveledUp>,
        MessageReader<gd_game::progress::PointsGained>,
    ),
    mut fresh: gd_game::session::NewSession,
    (mut state, mut indicator): (Local<HudPulses>, ResMut<LevelUpIndicator>),
    mut glows: Query<(
        Entity,
        &GlowNode,
        &mut Visibility,
        Option<&MaterialNode<super::additive::AdditiveMaterial>>,
    )>,
    mut plains: Query<(&PlainOfGlow, &mut Visibility), Without<GlowNode>>,
    (mut cache, mut mats): (
        ResMut<super::additive::AdditiveCache>,
        ResMut<Assets<super::additive::AdditiveMaterial>>,
    ),
) {
    if fresh.begun() {
        *state = HudPulses::default();
        indicator.0 = false;
    }
    let load = |field: &str| {
        data.db
            .get(MASTER)
            .and_then(|m| m.str(field).map(str::to_owned))
            .and_then(|p| data.db.get(&p))
            .map(ColorPulse::from_record)
    };
    if !state.loaded {
        state.loaded = true;
        state.level_up = load("playerPortraitLevelUpIndicatorColorPulse");
        state.buttons = [
            load("hudWindowButtonColorPulse"),
            load("hudWindowButtonColorPulse"),
        ];
    }
    let start = |state: &mut HudPulses, i: usize| {
        if !state.on[i] {
            let other = 1 - i;
            let running = (state.on[other])
                .then(|| state.buttons[other].clone())
                .flatten()
                .filter(ColorPulse::is_pulsing);
            if let Some(p) = state.buttons[i].as_mut() {
                p.reset();
                if let Some(o) = running {
                    p.sync_to(&o);
                }
            }
        }
        state.on[i] = true;
    };
    if leveled.read().count() > 0 {
        start(&mut state, 0);
    }
    for g in gained.read() {
        if g.skill {
            start(&mut state, 1);
            indicator.0 = true;
        }
        if g.attribute {
            start(&mut state, 0);
            indicator.0 = true;
        }
    }
    if panels.character && !state.open.0 {
        state.on[0] = false;
        indicator.0 = false;
    }
    if panels.skills && !state.open.1 {
        state.on[1] = false;
        indicator.0 = false;
    }
    state.open = (panels.character, panels.skills);
    let dt = time.delta_secs() * 1000.0;
    let level_up = state.level_up.as_mut().map(|p| {
        p.update(dt);
        p.color()
    });
    let mut button = [None, None];
    for i in 0..2 {
        if !state.on[i] {
            continue;
        }
        let Some(p) = state.buttons[i].as_mut() else {
            continue;
        };
        if p.is_pulsing() {
            p.update(dt);
            button[i] = Some(p.color());
        } else {
            state.on[i] = false;
        }
    }
    let color = |g: Glow| match g {
        Glow::Indicator => level_up.filter(|_| indicator.0),
        Glow::AttributeButton => level_up.filter(|_| state.on[0]),
        Glow::SkillButton => level_up.filter(|_| state.on[1]),
        Glow::CharacterMenu => button[0],
        Glow::SkillsMenu => button[1],
    };
    for (e, g, mut vis, shown) in &mut glows {
        let Some(c) = color(g.glow) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        vis.set_if_neq(Visibility::Inherited);
        let h = cache.get(&mut mats, &g.image, c);
        if shown.is_none_or(|m| m.0 != h) {
            commands.entity(e).try_insert(MaterialNode(h));
        }
    }
    for (p, mut vis) in &mut plains {
        vis.set_if_neq(if color(p.0).is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

/// A low health / energy bar's flash (UIPlayerHud: the life pulse +4632 with its ratio +4616 and flag +705260, the
/// energy pulse +3696 with +3688 / +705259).
#[derive(Clone, Debug)]
pub struct BarFlash {
    pulse: ColorPulse,
    at: f32,
    on: bool,
}

impl BarFlash {
    /// The bar's tint this frame for its fill `ratio`, advancing the pulse by `dt` ms ([`flash_bars`]).
    fn step(&mut self, ratio: f32, dt: f32) -> Option<Color> {
        let low = ratio <= self.at;
        if low && !self.on {
            self.pulse.reset();
        }
        self.on = low;
        (low && self.pulse.is_pulsing()).then(|| {
            self.pulse.update(dt);
            self.pulse.color()
        })
    }
}

/// The health and energy bars flash while low. `Grim Dawn.exe`: the HUD loader sub_1402139D0 reads
/// `hudHealthBarColorPulse` / `hudHealthBarStartFlashingAtRatio` and `hudManaBarColorPulse` /
/// `hudManaBarStartFlashingAtRatio` (stock: `hud_playerstatusbarcolorpulse.dbr`, grey 0.5 ↔ white every 500 ms, no
/// cycle count; 0.3 and 0.2); the update sub_14020D550 sets each bar's flag while its current / max ratio is at or
/// below the ratio, resetting the pulse when the flag turns on; the draw sub_14020A9D0 draws a flagged bar whose
/// pulse `IsPulsing` through the bar's coloured render with the pulse's colour (the fill tinted by it here), else
/// plainly. The fields are the same in both HUD tables (read from [`MASTER`]).
pub fn flash_bars(
    time: Res<Time>,
    data: Res<GameData>,
    vitals: Res<super::HudVitals>,
    mut fresh: gd_game::session::NewSession,
    mut state: Local<Option<[Option<BarFlash>; 2]>>,
    mut bars: Query<(&BarFill, &mut ImageNode)>,
) {
    if fresh.begun() {
        *state = None;
    }
    let flashes = state.get_or_insert_with(|| {
        let master = data.db.get(MASTER);
        let load = |pulse: &str, ratio: &str| {
            let m = master.as_ref()?;
            let r = data.db.get(m.str(pulse)?)?;
            Some(BarFlash {
                pulse: ColorPulse::from_record(r),
                at: m.f32(ratio)?,
                on: false,
            })
        };
        [
            load("hudHealthBarColorPulse", "hudHealthBarStartFlashingAtRatio"),
            load("hudManaBarColorPulse", "hudManaBarStartFlashingAtRatio"),
        ]
    });
    let dt = time.delta_secs() * 1000.0;
    let ratios = [
        vitals.health / vitals.health_max.max(1.0),
        vitals.energy / vitals.energy_max.max(1.0),
    ];
    let tints: [Color; 2] = std::array::from_fn(|i| {
        flashes[i]
            .as_mut()
            .and_then(|f| f.step(ratios[i], dt))
            .unwrap_or(Color::WHITE)
    });
    for (b, mut img) in &mut bars {
        let i = match b.stat {
            Stat::Health => 0,
            Stat::Energy => 1,
            _ => continue,
        };
        if img.color != tints[i] {
            img.color = tints[i];
        }
    }
}

/// The menu buttons of the classic HUD, in the engine's draw order.
const MENU_BUTTONS: [&str; 8] = [
    "hudMenuOptionsButton",
    "hudMiniMapButton",
    "hudAchievementsButton",
    "hudFactionButton",
    "hudCharacterButton",
    "hudSkillsButton",
    "hudPartyButton",
    "hudQuestButton",
];

/// The game-mode label at the upper left (`hudGameModeText`,
/// `hud_modelabel.dbr`): its tags (`Mode::label_tags_in`) localised and joined
/// with spaces (sub_140218AE0), at the record's position × scale
/// (`TextString` load sub_14025A160 rounds it), left / top.
fn mode_label(ui: &mut Ui, rec: &str, tags: &[&str]) {
    let Some(r) = ui.record(rec) else { return };
    let text = tags
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| ui.fonts.tag(t).to_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let (Some(style), false) = (r.str("style").map(str::to_owned), text.is_empty()) else {
        return;
    };
    let tb = TextBox::from_record(&r);
    let at = Vec2::new(ui.sc(tb.rect.x), ui.sc(tb.rect.y));
    ui.text_line(&style, at, &text, 1.0);
}

/// The local time as `GetTimeFormatA` writes it for the default (en-US)
/// locale: "h:mm:ss AM".
fn local_time() -> Option<String> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: `localtime_r` fills `tm` from `now`; both are valid locals.
    if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
        return None;
    }
    let h = tm.tm_hour % 12;
    Some(format!(
        "{}:{:02}:{:02} {}",
        if h == 0 { 12 } else { h },
        tm.tm_min,
        tm.tm_sec,
        if tm.tm_hour < 12 { "AM" } else { "PM" }
    ))
}

#[derive(Component)]
pub struct ClockRoot;

/// "Display Time" (`showTime`, `Options` bool 65): the HUD draws
/// `hudClockText` (`hud_clocklabel.dbr`, `style_clock_mono`) with
/// `LocalizationManager::GetFormattedTime` (Engine.dll 0x180143340:
/// `GetLocalTime` + `GetTimeFormatA` with the user locale's format) each
/// frame (sub_14020A9D0); without a game-mode label it takes the
/// label's place (sub_140218AE0). PORT: the en-US format; the HUD's other
/// widgets do not move down for it (sub_14020D550 offsets one by its height).
#[allow(clippy::too_many_arguments)]
pub fn clock(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    options: Res<gd_game::options::GameOptions>,
    values: Res<HudValues>,
    roots: Query<Entity, With<ClockRoot>>,
    real: Res<Time<Real>>,
    // (size, the shown second or None when off, labelled, classic HUD).
    mut built: Local<Rebuild<(UVec2, Option<i64>, bool, bool)>>,
    // (the last second read, its text): libc is read only when the second could have changed.
    mut tick: Local<(i64, Option<String>)>,
    mut assets: crate::ui::UiAssets,
) {
    if fresh.begun() {
        built.reset();
        *tick = (-1, None);
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let shown = options.flag("showTime");
    // PORT: read the clock once a second (the real clock's whole second advances), not every frame.
    let sec = real.elapsed_secs() as i64;
    if shown && sec != tick.0 {
        tick.0 = sec;
        tick.1 = local_time();
    }
    let labelled = !values.mode_tags[0].is_empty();
    let sig = (
        size,
        shown.then_some(tick.0),
        labelled,
        assets.classic_hud(),
    );
    if size.x == 0 || !built.changed(sig) {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    let Some(text) = tick.1.as_deref().filter(|_| shown) else {
        return;
    };
    let root = super::spawn_hud_overlay(&mut commands, ClockRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let Some(m) = ui.record(ui.hud_master()) else {
        return;
    };
    let field = |k: &str| m.str(k).unwrap_or("").to_string();
    let (Some(r), Some(label)) = (
        ui.record(&field("hudClockText")),
        ui.record(&field("hudGameModeText")),
    ) else {
        return;
    };
    let Some(style) = r.str("style").map(str::to_owned) else {
        return;
    };
    let tb = TextBox::from_record(if labelled { &r } else { &label });
    let at = Vec2::new(ui.sc(tb.rect.x), ui.sc(tb.rect.y));
    ui.text_line(&style, at, text, 1.0);
}

/// The HUD master field of the teleport button shown.
pub fn teleport_field(t: gd_game::riftgates::PortalButton) -> &'static str {
    if t.active {
        "hudActiveTeleportButton"
    } else {
        "hudTeleportButton"
    }
}

/// Screen rect of the teleport button (its up bitmap's size).
pub fn teleport_rect(ui: &mut Ui, t: gd_game::riftgates::PortalButton) -> Option<URect> {
    let to = toolbar_origin(ui)?;
    let rec = ui
        .record(ui.hud_master())?
        .str(teleport_field(t))?
        .to_string();
    let Widget::Button {
        up: Some(up), x, y, ..
    } = ui.widget(&rec)?
    else {
        return None;
    };
    let (_, sz) = ui.image(&up)?;
    Some(URect::new(
        to.x + ui.sc(x),
        to.y + ui.sc(y),
        ui.sc(sz.x as f32),
        ui.sc(sz.y as f32),
    ))
}

/// The HUD takes the cursor over any of its hit areas (sub_14020C140: the toolbar window,
/// the status / facade / region strips, the orbs and the experience bar, the buttons and slots), so a
/// click on the HUD background does not fall through to a world move / attack. Runs before the HUD
/// button systems ([`buttons`]); [`HudLayout::covers`] is the one hit test.
pub fn capture(
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<crate::hooks::Hooks>,
    layout: Res<HudLayout>,
    mut capture: ResMut<crate::inventory::UiCapture>,
) {
    let Ok(win) = window.single() else { return };
    if let Some(p) = super::cursor_px(&hooks, win)
        && layout.covers(p)
    {
        capture.0 = true;
    }
}

/// The HUD's buttons, one system mapping a [`HudLayout`] field to its action (the engine's HUD button
/// listener, sub_1402129E0). The press sounds and modal blocking are the registered
/// [`super::widgets::ButtonSound`] nodes' (`widgets.rs`, [`super::widgets::frontmost`]): the frontmost
/// registered rect under the cursor takes the press, so a modal window's blocker blocks the buttons
/// behind it. The weapon-swap / quickbar keys and the `GD_WEAPON_SWAP` / `GD_HUD_CLICK` hooks act here
/// too. PORT: on press (the level-up indicator on release, event 1), like the engine's toolbar buttons.
#[allow(clippy::too_many_arguments)]
pub fn buttons(
    layout: Res<HudLayout>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    (dialog, rift_map, bindings): (
        Res<gd_game::conversation::Conversation>,
        Res<gd_game::riftgates::RiftgateMap>,
        Res<super::keys::KeyBindings>,
    ),
    casters: Query<&gd_game::casting::Caster, With<crate::player::Player>>,
    mut row: ResMut<HotbarRow>,
    mut windows: super::window::Windows,
    indicator: Res<LevelUpIndicator>,
    mut folder: ResMut<super::menufolder::MenuFolder>,
    (hooks, time, mut fired, mut swap_hook): (
        Res<crate::hooks::Hooks>,
        Res<Time>,
        Local<usize>,
        Local<Option<Vec<f32>>>,
    ),
    (mut swap, mut compass, mut show_set): (
        MessageWriter<gd_game::weapon_sets::SwapWeapons>,
        MessageWriter<gd_game::compass::ToggleCompass>,
        MessageWriter<gd_game::hotbar::ShowSkillSet>,
    ),
    (button_sounds, gz): (
        Query<(&super::widgets::ButtonSound, &ChildOf, &InheritedVisibility)>,
        super::widgets::RootZ,
    ),
) {
    use super::window::Window;
    use gd_game::hotbar::ShowSkillSet;
    use gd_game::weapon_sets::SwapWeapons;
    let keys_free = dialog.view.is_none() && !rift_map.open;
    // `GD_WEAPON_SWAP` (hooks.rs): the character window's swap button pressed at those times.
    let swap_hook = swap_hook.get_or_insert_with(|| hooks.ui.weapon_swap.clone());
    if let Some(i) = swap_hook.iter().position(|t| time.elapsed_secs() >= *t) {
        swap_hook.remove(i);
        info!("hook: weapon swap");
        swap.write(SwapWeapons { from_window: true });
    }
    if bindings.just_pressed(super::keys::WEAPON_SWAP, &keys) && keys_free {
        swap.write(SwapWeapons { from_window: false });
    }
    // Quickbar Switch (43) / Show Alternate Hot Slot Set (62), sub_1402110D0: only while the
    // active skill set is 0 and the row below 2.
    let toggle = bindings.just_pressed(super::keys::QUICKBAR_SWITCH, &keys) && keys_free;
    let alternate = bindings.just_pressed(super::keys::SHOW_ALTERNATE_SET, &keys) && keys_free;
    if (toggle || alternate)
        && let Ok(caster) = casters.single()
        && caster.set == 0
        && row.0 < 2
    {
        if toggle && caster.shown != 0 {
            row.0 = 0;
            show_set.write(ShowSkillSet::Reset);
        } else if toggle {
            row.0 = (row.0 + 1) % 2;
        } else {
            row.0 = 0;
            show_set.write(ShowSkillSet::Next);
        }
    }
    // A button field's action. The row-select button shares the quickbar rule (sub_1402129E0 +35968).
    let mut press = |field: &str| match field {
        "hudButtonWeaponSwap" => {
            swap.write(SwapWeapons { from_window: false });
        }
        "hudCompassButton" => {
            compass.write(gd_game::compass::ToggleCompass);
        }
        "hotSlotSelectUpButton" => {
            if let Ok(caster) = casters.single()
                && caster.set == 0
                && row.0 < 2
            {
                let other_page = caster.shown != 0;
                if other_page || row.0 == 1 {
                    if !other_page {
                        row.0 = 0;
                    }
                    show_set.write(ShowSkillSet::Next);
                } else {
                    row.0 = 1;
                }
            }
        }
        "hudCharacterButton" => {
            windows.toggle(Window::Character);
            if windows.panels.character && windows.panels.tab == 0 {
                windows.panels.tab = 1;
            }
        }
        "hudSkillsButton" => windows.toggle(Window::Skills),
        "hudQuestButton" => windows.toggle(Window::Journal),
        "hudMiniMapButton" if !rift_map.open => windows.toggle(Window::Map),
        "hudFactionButton" => windows.toggle(Window::Factions),
        "hudAchievementsButton" => windows.toggle(Window::Achievements),
        "playerLevelUpAttributeButton" if !windows.panels.character => {
            windows.open(Window::Character);
            if windows.panels.tab == 0 {
                windows.panels.tab = 1;
            }
        }
        "playerLevelUpSkillButton" if !windows.panels.skills => windows.open(Window::Skills),
        super::menufolder::BUTTON => folder.open = !folder.open,
        LEVEL_UP_BUTTON if indicator.0 => {
            windows.panels.devotion = false;
            windows.journal.open = false;
            windows.map.open = false;
            windows.open(Window::Skills);
        }
        _ => {}
    };
    // `GD_HUD_CLICK` (hooks.rs): those clicks, in order.
    while let Some((at, field)) = hooks.ui.hud_click.get(*fired)
        && time.elapsed_secs() >= *at
    {
        info!("hook: click {field}");
        press(field);
        *fired += 1;
    }
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    // The level-up indicator (a hidden button, no registered sound) acts on its release (event 1).
    if indicator.0
        && let Some(r) = layout.rect(LEVEL_UP_BUTTON)
        && super::contains(&r, p)
    {
        if mouse.just_released(MouseButton::Left) {
            press(LEVEL_UP_BUTTON);
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // The frontmost registered rect takes the press (a modal's blocker blocks the buttons behind it).
    let Some(front) = super::widgets::frontmost(p, &button_sounds, &gz) else {
        return;
    };
    // Which action button it is: its [`HudLayout`] rect is the registered rect. A button only registers
    // a sound while it is drawn, so the draw's gates (folder open, points to spend) already hold.
    const ACTION_FIELDS: [&str; 12] = [
        "hudButtonWeaponSwap",
        "hudCompassButton",
        "hotSlotSelectUpButton",
        "hudCharacterButton",
        "hudSkillsButton",
        "hudQuestButton",
        "hudMiniMapButton",
        "hudFactionButton",
        "hudAchievementsButton",
        "playerLevelUpAttributeButton",
        "playerLevelUpSkillButton",
        super::menufolder::BUTTON,
    ];
    if let Some(field) = ACTION_FIELDS
        .iter()
        .copied()
        .find(|f| layout.rect(f) == Some(front.rect))
    {
        press(field);
    }
}

/// The hotbar row shown (the HUD's +29424: 0 or 1; 2 / 3 are the gamepad's), which the hot slot keys and the
/// hotbar use. `Grim Dawn.exe`: the key handler sub_140026910 reads the hot slot keys' row from it
/// (`a1+2132`); it starts at 0 with the HUD and is not saved.
#[derive(Resource, Default)]
pub struct HotbarRow(pub usize);

/// A root node that Toggle UI leaves shown ([`toggle_ui`]): the game menu, the options window, the cursor, the
/// cinematic text, the credits and the dialog manager's prompt boxes (`confirm.rs`, each in a root of its own).
#[derive(Component)]
pub struct KeepsShown;

/// A root node [`toggle_ui`] hid, with its display before.
#[derive(Component)]
pub struct ToggledOff(Display);

/// Toggle UI (action 60, default `]`) hides the HUD and the windows, and shows them again. `Grim Dawn.exe`:
/// the HUD key action sub_140211AF0 case 60 flips HUD +706960 (1 from the constructor sub_140205BE0, read only by
/// the draw); while it is 0 the HUD draw sub_14020A9D0 skips everything but the item spawn window (+581752), the game
/// menu (+303872, `hudExitWindow`), the options window (+319224) and the dialog manager's prompt boxes (+29560).
/// Input is not gated: the hidden windows still take their keys and clicks. PORT: a hidden root gets
/// `Display::None`; the roots flagged [`KeepsShown`] stay; a prompt box (`confirm.rs`) is drawn in
/// a [`KeepsShown`] root of its own, so it stays while the window that asked hides. The flag lives for the session (the HUD's lifetime).
#[allow(clippy::type_complexity)]
pub fn toggle_ui(
    mut commands: Commands,
    (keys, bindings, dialog, rift_map): (
        Res<ButtonInput<KeyCode>>,
        Res<super::keys::KeyBindings>,
        Res<gd_game::conversation::Conversation>,
        Res<gd_game::riftgates::RiftgateMap>,
    ),
    mut roots: Query<
        (Entity, &mut Node, Option<&ToggledOff>),
        (Without<ChildOf>, Without<KeepsShown>),
    >,
    mut hidden: Local<bool>,
    mut fresh: gd_game::session::NewSession,
) {
    if fresh.begun() {
        *hidden = false;
    }
    let mut flipped = false;
    if bindings.just_pressed(super::keys::TOGGLE_UI, &keys)
        && dialog.view.is_none()
        && !rift_map.open
    {
        *hidden = !*hidden;
        flipped = true;
    }
    // Shown and not just unhidden: every root is as built (the unhide frame restored them all).
    if !*hidden && !flipped {
        return;
    }
    for (e, mut node, off) in &mut roots {
        if *hidden {
            if node.display == Display::None {
                continue;
            }
            if off.is_none() {
                commands.entity(e).try_insert(ToggledOff(node.display));
            }
            node.display = Display::None;
        } else if let Some(off) = off {
            node.display = off.0;
            commands.entity(e).try_remove::<ToggledOff>();
        }
    }
}

/// The classic HUD's level-up indicator button field.
pub const LEVEL_UP_BUTTON: &str = "playerPortraitLevelUpIndicatorButton";

/// The screen rect of the classic HUD's hidden level-up indicator button (a 60 x 60 button at the portrait
/// origin; the orb HUD's table has none).
pub fn level_up_button_rect(ui: &Ui) -> Option<URect> {
    let btn = ui
        .record(ui.hud_master())?
        .str(LEVEL_UP_BUTTON)
        .and_then(|b| ui.record(b))?;
    let po = portrait_origin(ui)?;
    let f = |k: &str| btn.f32(k).unwrap_or(0.0);
    Some(URect::new(
        po.x + ui.sc(f("bitmapPositionX")),
        po.y + ui.sc(f("bitmapPositionY")),
        ui.sc(f("buttonXSize")),
        ui.sc(f("buttonYSize")),
    ))
}

/// Screen rect of a HUD button field: its up bitmap at `origin` + the record position × scale.
pub fn button_rect_at(ui: &mut Ui, field: &str, origin: Vec2) -> Option<URect> {
    let rec = ui.record(ui.hud_master())?.str(field)?.to_string();
    let Widget::Button {
        up: Some(up), x, y, ..
    } = ui.widget(&rec)?
    else {
        return None;
    };
    let (_, sz) = ui.image(&up)?;
    Some(URect::new(
        origin.x + ui.sc(x),
        origin.y + ui.sc(y),
        ui.sc(sz.x as f32),
        ui.sc(sz.y as f32),
    ))
}

/// Top-left of the `hudToolbar` anchor (screen px).
pub fn toolbar_origin(ui: &Ui) -> Option<Vec2> {
    let m = ui.record(ui.hud_master())?;
    let toolbar = ui
        .record(m.str("hudToolbar")?)
        .map(|r| WindowDef::from_record(&r))
        .unwrap_or_default();
    let t = toolbar.screen_rect(ui.screen.x, ui.screen.y, ui.s);
    Some(Vec2::new(t.x, t.y))
}

/// Screen rect of a hot slot record (`slot` = `hudHotSlot1`, `hudEvadeButton`, ...).
pub fn slot_rect(ui: &Ui, field: &str) -> Option<URect> {
    let to = toolbar_origin(ui)?;
    let rec = ui.record(ui.hud_master())?.str(field)?.to_string();
    let Widget::HotSlot { x, y, w, h, .. } = ui.widget(&rec)? else {
        return None;
    };
    let (w, h) = if w <= 0.0 || h <= 0.0 {
        (32.0, 32.0)
    } else {
        (w, h)
    };
    Some(URect::new(
        to.x + ui.sc(x),
        to.y + ui.sc(y),
        ui.sc(w),
        ui.sc(h),
    ))
}

/// Top-left of the `hudPortraitAndStatsSummary` anchor (screen px), placed
/// directly above the toolbar as in [`build`].
pub fn portrait_origin(ui: &Ui) -> Option<Vec2> {
    let m = ui.record(ui.hud_master())?;
    let toolbar = ui
        .record(m.str("hudToolbar")?)
        .map(|r| WindowDef::from_record(&r))
        .unwrap_or_default();
    let t = toolbar.screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let portrait = ui
        .record(m.str("hudPortraitAndStatsSummary")?)
        .map(|r| WindowDef::from_record(&r))
        .unwrap_or_default();
    let mut p = portrait.screen_rect(ui.screen.x, ui.screen.y, ui.s);
    p.y = t.y - p.h;
    Some(Vec2::new(p.x, p.y))
}

/// A HUD button's state bitmaps ([`state_button`]); [`button_states`] shows one per cursor + press.
#[derive(Component)]
pub struct HudButton {
    /// The HUD master field ([`super::gamemenu::GameMenu::blocks_hud`]).
    field: &'static str,
    pub rect: URect,
    up: Handle<Image>,
    over: Option<Handle<Image>>,
    down: Option<Handle<Image>>,
    disabled: Option<Handle<Image>>,
    /// The button works (else its disabled bitmap, no hover / press look).
    enabled: bool,
    /// A toggle showing its down bitmap while its window is open.
    toggled: bool,
}

/// Draws a HUD button with its four state bitmaps ([`HudButton`]) and registers its sounds + modal
/// blocking ([`super::widgets::ButtonSound`]). `ButtonStatic::Draw` sub_140123C50: the down
/// bitmap while the down flag is set, the disabled bitmap while disabled, `bitmapNameInFocus` while
/// hovered, else up. `enabled`: the button works; `toggled`: a toggle down while its window is open.
/// PORT: the loot filter button registers no sound here — `lootfilter.rs` plays its own (the up
/// sound on close too).
fn state_button(
    ui: &mut Ui,
    field: &'static str,
    rec: &str,
    origin: Vec2,
    enabled: bool,
    toggled: bool,
) -> Option<(Entity, URect)> {
    let Widget::Button {
        up: Some(up),
        over,
        down,
        disabled,
        x,
        y,
        ..
    } = ui.widget(rec)?
    else {
        return None;
    };
    let (up_h, size) = ui.image(&up)?;
    let load =
        |ui: &mut Ui, p: &Option<String>| p.as_deref().and_then(|p| ui.image(p)).map(|(h, _)| h);
    let over_h = load(ui, &over);
    let down_h = load(ui, &down);
    let dis_h = load(ui, &disabled);
    let r = URect::new(
        origin.x + ui.sc(x),
        origin.y + ui.sc(y),
        ui.sc(size.x as f32),
        ui.sc(size.y as f32),
    );
    let initial = if !enabled {
        dis_h.as_ref().unwrap_or(&up_h)
    } else if toggled {
        down_h.as_ref().or(over_h.as_ref()).unwrap_or(&up_h)
    } else {
        &up_h
    };
    let e = ui.quad(initial.clone(), r, None, Color::WHITE);
    ui.commands.entity(e).insert(HudButton {
        field,
        rect: r,
        up: up_h,
        over: over_h,
        down: down_h,
        disabled: dis_h,
        enabled,
        toggled,
    });
    if enabled
        && field != "hudLootFilterButton"
        && let Some(brec) = ui.record(rec)
    {
        ui.record_button_sound(r, &brec);
    }
    Some((e, r))
}

/// Shows each HUD button's state bitmap in place (`ButtonStatic::Draw` sub_140123C50): the
/// down bitmap while held over it (or toggled down), the disabled bitmap while unusable,
/// `bitmapNameInFocus` while hovered, else up. Per cursor + `ButtonInput`, no rebuild and no second
/// overlay (the character-window buttons' hover is `rollover.rs`'s `FocusRoot`). Under the game menu
/// / options window no button hovers but the options button ([`super::gamemenu::GameMenu::blocks_hud`]).
pub fn button_states(
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<crate::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    (pause, menu): (
        Res<gd_game::pause::GamePause>,
        Res<super::gamemenu::GameMenu>,
    ),
    mut q: Query<(&HudButton, &mut ImageNode)>,
) {
    let p = window
        .single()
        .ok()
        .and_then(|w| super::cursor_px(&hooks, w));
    let pressed = buttons.pressed(MouseButton::Left);
    for (b, mut img) in &mut q {
        let hover =
            p.is_some_and(|p| super::contains(&b.rect, p)) && !menu.blocks_hud(&pause, b.field);
        let h = if !b.enabled {
            b.disabled.as_ref().unwrap_or(&b.up)
        } else if b.toggled || (hover && pressed) {
            b.down.as_ref().or(b.over.as_ref()).unwrap_or(&b.up)
        } else if hover {
            b.over.as_ref().unwrap_or(&b.up)
        } else {
            &b.up
        };
        if img.image != *h {
            img.image = h.clone();
        }
    }
}

/// The hot slots' key labels by HUD master field: the bound key's name (sub_1401E6C90 reads
/// the action's primary, else secondary, key code and asks the input device for its name), the mouse
/// slots the `tagHUD_LeftMouse` / `tagHUD_RightMouse` tags. A rebind changes them (a layout change,
/// [`super::HudSig`]); [`super::view::collect`] publishes them.
pub fn slot_labels(
    bindings: &super::keys::KeyBindings,
    tags: &gd_formats::text::TagTable,
) -> std::collections::BTreeMap<&'static str, String> {
    let bound = |action: u32| {
        let [p, s] = bindings.get(action);
        match if p != 0 { p } else { s } {
            0 => tags
                .get("tagKeybindUnassigned")
                .unwrap_or("tagKeybindUnassigned")
                .to_owned(),
            c => super::keys::key_name(c),
        }
    };
    let mut m = std::collections::BTreeMap::new();
    m.insert("hudEvadeButton", bound(super::keys::EVADE));
    m.insert("hudPriPotionButton", bound(super::keys::DRINK_HEALTH));
    m.insert("hudSecPotionButton", bound(super::keys::DRINK_ENERGY));
    for (i, f) in HOT_SLOTS.into_iter().enumerate() {
        m.insert(f, bound(super::keys::HOT_SLOT_FIRST + i as u32));
    }
    m.insert(
        "hudPriSkillButton",
        tags.get("tagHUD_LeftMouse").unwrap_or("L").to_owned(),
    );
    m.insert(
        "hudSecSkillButton",
        tags.get("tagHUD_RightMouse").unwrap_or("R").to_owned(),
    );
    m
}

/// A hot slot's persistent state highlight (sub_1401E6C90 / `HotSlotOptionSkill::GetStatus`
/// @0x18031c140: low energy → `highlightBlue`, wrong weapons → `highlightRed`, outside its form /
/// not learned → the dark rect). PORT: an active toggle / aura shows the blue highlight too — the
/// engine's `GetStatus` reads only the availability, the port makes the "on" state visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlotLook {
    /// Ready: no highlight.
    #[default]
    None,
    /// An active toggle / aura, or not enough energy: `highlightBlue`.
    Blue,
    /// The worn weapons don't suit it: `highlightRed`.
    Red,
    /// Unusable (outside its shapeshift form / not learned): the dark rect (0, 0, 0, 0.75).
    Dark,
}

/// A shown hot slot's state overlay ([`super::view::collect`] fills it, [`hot_slot_states`] draws it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotSlotState {
    /// The HUD master field.
    pub field: &'static str,
    pub look: SlotLook,
    /// Whether the slot can be activated now (the hover rect: green usable, red not).
    pub usable: bool,
    /// The charge count to show (charged weapon-pool skills), none otherwise.
    pub charges: Option<u32>,
}

#[derive(Component)]
pub struct SlotStateRoot;

/// A slot's `highlightBlue` / `highlightRed` image node ([`hot_slot_states`]).
#[derive(Component)]
pub(crate) struct SlotHighlight {
    field: &'static str,
    rect: URect,
    blue: Option<Handle<Image>>,
    red: Option<Handle<Image>>,
}

/// A slot's state / hover rect node ([`hot_slot_states`]).
#[derive(Component)]
pub(crate) struct SlotRect {
    field: &'static str,
    rect: URect,
}

/// A slot's charge-count text ([`hot_slot_states`]).
#[derive(Component)]
pub(crate) struct SlotCharge {
    field: &'static str,
    shown: Option<u32>,
}

/// The hot slots' state overlays (sub_1401E6770 / sub_1401E6C90): over the icon and the
/// cooldown wipe, one of the hover rect (green usable / red not), `highlightBlue`, `highlightRed` or
/// the dark rect, plus the charge count. Built per layout, animated in place from
/// [`super::HudVitals::hot_slots`] (like the cooldown wipe). The hover rect takes priority over the
/// persistent highlight (the draw's order). Under the game menu / options window no slot hovers
/// ([`super::gamemenu::GameMenu::blocks_hud`]).
#[allow(clippy::too_many_arguments)]
pub fn hot_slot_states(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    vitals: Res<super::HudVitals>,
    layout: Res<HudLayout>,
    roots: Query<(Entity, &super::RootFrame), With<SlotStateRoot>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    mut last: Local<Option<(UVec2, bool, u32)>>,
    mut assets: crate::ui::UiAssets,
    (pause, menu): (
        Res<gd_game::pause::GamePause>,
        Res<super::gamemenu::GameMenu>,
    ),
    (mut highlights, mut rects, mut charges, parents): (
        Query<(&SlotHighlight, &mut ImageNode, &mut Visibility)>,
        Query<(&SlotRect, &mut BackgroundColor, &mut Visibility), Without<SlotHighlight>>,
        Query<(Entity, &mut SlotCharge, &mut super::LiveText)>,
        Query<&ChildOf>,
    ),
) {
    if fresh.begun() {
        *last = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let classic = assets.classic_hud();
    // The UI scale too: closing Options after a scale change rebuilds the HUD (`HudLayout`), so the rects follow.
    let scale = assets.scale(size).to_bits();
    let same = *last == Some((size, classic, scale));
    let kept = super::retire_roots(
        &mut commands,
        roots.iter().map(|(e, f)| (e, f.0)),
        frame.0,
        !same,
        false,
    );
    let cursor = super::cursor_px(&assets.hooks, win);
    let state = |field: &str| vitals.hot_slots.iter().find(|s| s.field == field);
    if same {
        for (h, mut img, mut vis) in &mut highlights {
            let hover = cursor.is_some_and(|p| super::contains(&h.rect, p))
                && !menu.blocks_hud(&pause, h.field);
            let handle = match state(h.field).map(|s| s.look) {
                Some(SlotLook::Blue) if !hover => h.blue.as_ref(),
                Some(SlotLook::Red) if !hover => h.red.as_ref(),
                _ => None,
            };
            if let Some(handle) = handle {
                if img.image != *handle {
                    img.image = handle.clone();
                }
                vis.set_if_neq(Visibility::Inherited);
            } else {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
        for (r, mut bg, mut vis) in &mut rects {
            let s = state(r.field);
            let hover = cursor.is_some_and(|p| super::contains(&r.rect, p))
                && !menu.blocks_hud(&pause, r.field);
            let color = if hover {
                Some(if s.is_some_and(|s| s.usable) {
                    Color::srgba(0.0, 1.0, 0.0, 0.5)
                } else {
                    Color::srgba(1.0, 0.0, 0.0, 0.5)
                })
            } else if s.is_some_and(|s| s.look == SlotLook::Dark) {
                Some(Color::srgba(0.0, 0.0, 0.0, 0.75))
            } else {
                None
            };
            if let Some(color) = color {
                bg.set_if_neq(BackgroundColor(color));
                vis.set_if_neq(Visibility::Inherited);
            } else {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
        for (e, mut c, mut live) in &mut charges {
            if kept != Some(parents.root_ancestor(e)) {
                continue;
            }
            let count = state(c.field).and_then(|s| s.charges);
            if c.shown != count {
                c.shown = count;
                let text = count.map(|n| n.to_string()).unwrap_or_default();
                live.set(e, &text, &mut commands, &data, &mut assets);
            }
        }
        return;
    }
    *last = Some((size, classic, scale));
    let root = super::spawn_frame_root(&mut commands, SlotStateRoot, frame.0, 1);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        size.as_vec2(),
    );
    let Some(m) = ui.record(ui.hud_master()) else {
        return;
    };
    for field in HOT_SLOTS.into_iter().chain(LAYOUT_SLOTS) {
        let Some(r) = layout.rect(field) else {
            continue;
        };
        let Some(rec) = m.str(field).map(str::to_owned) else {
            continue;
        };
        let highlight = |ui: &mut Ui, key: &str| {
            ui.record(&rec)
                .and_then(|b| b.str(key).map(str::to_owned))
                .and_then(|p| ui.image(&p))
                .map(|(h, _)| h)
        };
        let blue = highlight(&mut ui, "highlightBlue");
        let red = highlight(&mut ui, "highlightRed");
        let e = ui
            .commands
            .spawn((
                ui.node(r),
                ImageNode::default(),
                Visibility::Hidden,
                SlotHighlight {
                    field,
                    rect: r,
                    blue,
                    red,
                },
            ))
            .id();
        ui.commands.entity(ui.root).add_child(e);
        let e = ui
            .commands
            .spawn((
                ui.node(r),
                BackgroundColor(Color::NONE),
                Visibility::Hidden,
                SlotRect { field, rect: r },
            ))
            .id();
        ui.commands.entity(ui.root).add_child(e);
        if let Some(Widget::HotSlot { text: tb, .. }) = ui.widget(&rec) {
            let at = super::TextAt::Box {
                tb,
                origin: Vec2::new(r.x, r.y),
            };
            let e = ui.live_text(at, "");
            ui.commands
                .entity(e)
                .insert(SlotCharge { field, shown: None });
        }
    }
}

/// A skill's icon for a hot slot record: `isCircular` slots (the evade and
/// potion buttons in stock data) show the skill's `skillUpRoundBitmapName`
/// (sub_1401E6C90: hot slot +640 picks the option's
/// `GetRoundBitmapNameUp`, vtable +0x60, over `GetBitmapNameUp`, +0x38;
/// `HotSlotOptionPotionSkill` inherits the skill's round name), the others
/// `skillUpBitmapName`. PORT: the plain icon if a skill has no round one.
fn slot_icon(ui: &Ui, rec: &str, skill: &str, book: &gd_sim::skills::SkillBook) -> Option<String> {
    let round = matches!(ui.widget(rec), Some(Widget::HotSlot { circular: true, .. }));
    let r = ui.data.db.get(skill)?;
    // `HotSlotOptionPotionSkill::GetBitmapNameUp` @0x180276f00 (the non-round name,
    // sub_1401E6C90): a `Skill_ChargePotion`'s own bitmap (`UpdatePotionBitmap`'s combo
    // bitmap) when it has one, else the skill's.
    let combo = (!round)
        .then(|| potion_bitmap(&ui.data.db, skill, book))
        .flatten()
        .map(|b| b.texture)
        .filter(|t| !t.is_empty());
    combo.or_else(|| {
        round
            .then(|| r.str("skillUpRoundBitmapName"))
            .flatten()
            .or(r.str("skillUpBitmapName"))
            .map(str::to_owned)
    })
}

/// A potion slot's combo bitmap, mask and mask offsets (`Skill_ChargePotion::GetPotionBitmap` /
/// `GetPotionMask` / `GetMaskTopOffset` / `GetMaskBottomOffset`,
/// `HotSlotOptionPotionSkill::GetBitmapMask` @0x180277020, `GetMaskTopOffset` @0x1802770a0).
pub fn potion_bitmap(
    db: &gd_formats::vfs::Database,
    skill: &str,
    book: &gd_sim::skills::SkillBook,
) -> Option<gd_sim::potions::PotionBitmap> {
    let tree = gd_sim::potions::trees(db)
        .into_iter()
        .find(|t| t.potion.eq_ignore_ascii_case(skill))?;
    gd_sim::potions::potion_bitmap(db, book, &tree)
}

/// Hot slots: the slot art is part of the toolbar bitmaps; the skill icon
/// (scaled to the slot) and the key label (`keyTextTable` text box,
/// relative to the slot) are drawn.
fn hot_slot(ui: &mut Ui, rec: &str, origin: Vec2, label: Option<&str>, icon: Option<&str>) {
    let Some(Widget::HotSlot {
        x,
        y,
        w,
        h,
        key_text,
        ..
    }) = ui.widget(rec)
    else {
        return;
    };
    // The round evade button has no bitmap size: 32 × 32.
    let (w, h) = if w <= 0.0 || h <= 0.0 {
        (32.0, 32.0)
    } else {
        (w, h)
    };
    if let Some((img, _)) = icon.and_then(|i| ui.image(i)) {
        let r = URect::new(origin.x + ui.sc(x), origin.y + ui.sc(y), ui.sc(w), ui.sc(h));
        ui.quad(img, r, None, Color::WHITE);
    }
    if let Some(label) = label {
        key_label(ui, rec, origin, label, key_text);
    }
}

/// A hot slot's key label (`keyTextTable` text box, relative to the slot);
/// `key_text` = the slot widget's table if already read.
pub fn key_label(ui: &mut Ui, rec: &str, origin: Vec2, label: &str, key_text: Option<String>) {
    let Some(Widget::HotSlot {
        x,
        y,
        key_text: own,
        ..
    }) = ui.widget(rec)
    else {
        return;
    };
    let Some(kt) = key_text.or(own) else { return };
    let Some(Widget::HotSlot { text: tb, .. }) = ui.widget(&kt) else {
        return;
    };
    let o = origin + Vec2::new(ui.sc(x), ui.sc(y));
    ui.text(&tb, o, label);
}

fn level_box(ui: &mut Ui, tb: &TextBox, origin: Vec2, level: u32) {
    ui.text(tb, origin, &level.to_string());
}

pub(super) fn bar_field(ui: &mut Ui, rec: &str, origin: Vec2, stat: Stat) {
    let Some(Widget::BarGraph(bar)) = ui.widget(rec) else {
        return;
    };
    let empty = bar.empty.as_deref().and_then(|p| ui.image(p));
    let Some((full, full_size)) = bar.full.as_deref().and_then(|p| ui.image(p)) else {
        return;
    };
    let rect = bar_widget_rect(ui, &bar, origin, empty.as_ref().map(|e| e.1), full_size);
    if bar.render_background
        && let Some((h, _)) = &empty
    {
        ui.quad(h.clone(), rect, None, Color::WHITE);
    }
    let fill = ui.quad(
        full,
        rect,
        Some(URect::new(0.0, 0.0, 1.0, 1.0)),
        Color::WHITE,
    );
    // The cap is masked by the empty bitmap (none without one,
    // sub_140120690); see `masked.rs`.
    let cap = match (bar.cap.as_deref().and_then(|p| ui.image(p)), &empty) {
        (Some((h, size)), Some((mask, _))) => {
            Some((super::masked::quad(ui, h, mask.clone(), rect, rect), size))
        }
        _ => None,
    };
    // Health / energy orbs: "current/max" over the orb in the bar's
    // `FontStyle` (updated per frame by `orb_texts`).
    if matches!(stat, Stat::Health | Stat::Energy)
        && let Some(style) = bar.font_style.clone()
    {
        let e = ui
            .commands
            .spawn((
                ui.node(rect),
                ImageNode::default(),
                Visibility::Hidden,
                OrbText {
                    stat,
                    style,
                    rect,
                    shown: None,
                    scale: ui.s,
                    inv: ui.inv,
                },
            ))
            .id();
        ui.commands.entity(ui.root).add_child(e);
    }
    let comp = BarFill {
        bar,
        stat,
        rect,
        full_size,
        cap,
        inv: ui.inv,
        scale: ui.s,
    };
    ui.commands.entity(fill).insert(comp);
}

fn bar_widget_rect(
    ui: &Ui,
    bar: &BarGraph,
    origin: Vec2,
    empty: Option<UVec2>,
    full: UVec2,
) -> URect {
    let (bw, bh) = bar.base_size(empty.map(|e| (e.x, e.y)), Some((full.x, full.y)));
    URect::new(
        origin.x + ui.sc(bar.x),
        origin.y + ui.sc(bar.y),
        ui.sc(bw),
        ui.sc(bh),
    )
}

/// Screen rect of a HUD master bar field (`hudHealthBar`, `hudManaBar` at the
/// portrait anchor, `hudExperienceBar` at the toolbar), as [`build`] draws it.
pub fn bar_rect(ui: &mut Ui, field: &str) -> Option<URect> {
    let origin = if field == "hudExperienceBar" {
        toolbar_origin(ui)?
    } else {
        portrait_origin(ui)?
    };
    let rec = ui.record(ui.hud_master())?.str(field)?.to_string();
    let Widget::BarGraph(bar) = ui.widget(&rec)? else {
        return None;
    };
    let empty = bar.empty.as_deref().and_then(|p| ui.image(p)).map(|e| e.1);
    let full = bar.full.as_deref().and_then(|p| ui.image(p))?.1;
    Some(bar_widget_rect(ui, &bar, origin, empty, full))
}

/// The floating minimap's area name (`UIPlayerHud::ToggleCompass`
/// 0x140209DB0, mode 3; the map, its frame and icons are `localmap.rs`).
///
/// The label is the minimap's `TextString` (record
/// `map_regionnamedisplaycompass.dbr` loaded by sub_140175640; text =
/// `Engine::GetAreaNameTag` localised, sub_140174FE0): in mode 3 it is
/// moved to x = W − (width + 20), y = the HUD master's `floatingAreaNameY`
/// (unscaled), and its alignment set to right / top (+52 = 1, +56 = 0), so
/// the name ends 20 px from the right edge, above the map. Width =
/// round(`textBoxXSize` × scale) (sub_14025A160).
fn minimap(ui: &mut Ui, area_tag: &str, name_y: f32) {
    if area_tag.is_empty() {
        return;
    }
    let Some(r) = ui.record(AREA_NAME) else {
        return;
    };
    let Some(style) = r.str("style").map(str::to_owned) else {
        return;
    };
    let name = ui.fonts.tag(area_tag).to_owned();
    let w = ui.text_width(&style, &name);
    ui.text_line(
        &style,
        Vec2::new(ui.screen.x - 20.0 - w, name_y),
        &name,
        1.0,
    );
}

/// The bars' fills and caps from the current values: only what changed is written (an unchanged node keeps
/// the UI layout and image extraction from redoing it).
pub fn animate_bars(
    time: Res<Time>,
    (values, vitals): (Res<HudValues>, Res<super::HudVitals>),
    mut bars: Query<(&BarFill, &mut Node, &mut ImageNode, &mut Visibility)>,
    mut caps: Query<(&mut Node, &mut Visibility), Without<BarFill>>,
) {
    for (b, mut node, mut img, mut vis) in &mut bars {
        let ratio = b.stat.ratio(&values, &vitals);
        let frame = b
            .bar
            .anim
            .map_or(0, |a| (time.elapsed_secs() * a.fps) as u32);
        let src = b.bar.fill_src(b.full_size.x, b.full_size.y, ratio, frame);
        let dst = b.bar.fill_dst(b.rect, ratio);
        vis.set_if_neq(if ratio > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        super::set_node_rect(&mut node, dst, b.inv);
        super::set_image_src(&mut img, src);
        // Cap (sub_140120690): the cap bitmap's height (vertical;
        // its width horizontal) centred on the fill edge, across the whole
        // widget, masked by the empty bitmap over the widget rect
        // (`masked.rs`), so it stays inside the orb at any level.
        if let Some((ce, csize)) = b.cap
            && let Ok((mut cn, mut cv)) = caps.get_mut(ce)
        {
            cv.set_if_neq(if ratio > 0.0 && ratio < 1.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            let (cw, ch) = (
                (csize.x as f32 * b.scale).round(),
                (csize.y as f32 * b.scale).round(),
            );
            let cap = if b.bar.vertical {
                let edge = if b.bar.reverse { dst.y + dst.h } else { dst.y };
                URect::new(b.rect.x, edge - ch * 0.5, b.rect.w, ch)
            } else {
                let edge = if b.bar.reverse { dst.x } else { dst.x + dst.w };
                URect::new(edge - cw * 0.5, b.rect.y, cw, b.rect.h)
            };
            super::set_node_rect(&mut cn, cap, b.inv);
        }
    }
}

/// The number shown on a health / energy orb.
#[derive(Component)]
pub struct OrbText {
    stat: Stat,
    style: String,
    rect: URect,
    /// The (current, max) drawn.
    shown: Option<(i64, i64)>,
    scale: f32,
    inv: f32,
}

pub fn orb_texts(
    values: Res<super::HudVitals>,
    data: Res<GameData>,
    mut fonts: ResMut<super::text::FontCache>,
    mut font_assets: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    mut q: Query<(&mut OrbText, &mut Node, &mut ImageNode, &mut Visibility)>,
) {
    for (mut o, mut node, mut img, mut vis) in &mut q {
        let (cur, max) = match o.stat {
            Stat::Health => (values.health, values.health_max),
            _ => (values.energy, values.energy_max),
        };
        // Truncated like the character window (`truncf` / int casts,
        // GrimDawn.exe sub_14013D9D0).
        let numbers = (cur.max(0.0) as i64, max as i64);
        if o.shown == Some(numbers) {
            continue;
        }
        o.shown = Some(numbers);
        let text = format!("{}/{}", numbers.0, numbers.1);
        let Some(mut style) = fonts.style(&data, &o.style) else {
            continue;
        };
        // PORT: the orb numbers are white in the real HUD (the style's red / green is not used).
        style.color0 = [1.0; 3];
        style.color1 = [1.0; 3];
        let Some(font) = fonts.font(&data, &style.font, &mut font_assets) else {
            continue;
        };
        let super::text::LoadedFont::Bitmap { font, atlas } = &*font else {
            continue;
        };
        let Some(ti) = fonts.rasterize_cached(
            &mut images,
            font,
            atlas,
            &style,
            style.scaled_size(o.scale),
            &text,
        ) else {
            continue;
        };
        let x = o.rect.x + (o.rect.w - ti.layout.width as f32) * 0.5 + ti.offset.x;
        let y = o.rect.y + (o.rect.h - ti.layout.height as f32) * 0.5 + ti.offset.y;
        super::set_node_rect(
            &mut node,
            URect::new(x.round(), y.round(), ti.size.x as f32, ti.size.y as f32),
            o.inv,
        );
        img.image = ti.image.clone();
        vis.set_if_neq(Visibility::Inherited);
    }
}
