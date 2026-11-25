//! Pet portraits (upper left of the HUD) and their right-click menu. The
//! portraits show `gd_game::pets::portrait_pets` (which pets, in which
//! order); clicks toggle the pet selection ([`crate::state::pets::PetSelection`])
//! and the menu sends [`SetPetStance`] / [`DisbandPet`]. Rules: `gd_game::pets`.
//!
//! Layout, `Grim Dawn.exe` (panel loader `PlayerStatusPanel::Load`, positions
//! `PlayerStatusPanel::LayoutPortraits`, portrait loader `PlayerPortrait::Load`, setup `PlayerPortrait::Setup`, update
//! `PlayerPortrait::Update`, draw `PlayerPortrait::Draw`, mouse `PlayerPortrait::HandleMouseEvent`, menu
//! `PlayerPortrait::OpenPetMenu` / `PlayerPortrait::OnMenuPick`), records `records/ui/hud/
//! hud_playerstatusdisplay.dbr` and `hud_pet{normal,aggressive,defensive}.dbr`:
//! - portrait `i` (pets first, then party members, none in single player) at
//!   (`initialPanelPositionX`·s, `initialPanelPositionY`·s + i·`panelDistance`·s):
//!   with `windowScreenAlignmentY Bottom` the loader's `H − (H − y·s)` puts
//!   the column 120·s below the top edge;
//! - drawn in this order, relative to the portrait: the health and mana bar
//!   backgrounds (`PetHealthBar*` / `PetManaBar*` rects ·s, black), the
//!   health fill (red, width × life / max life), the mana fill (0, .64, .28;
//!   width × energy / max energy), the pet's `StatusIcon` at (3, 3)·s (its
//!   `StatusIconRed` while dead or for 500 ms after losing life), the frame
//!   `BackgroundImagePet`, the selection frame `BackgroundImagePetHighlight`
//!   at `BackgroundImagePetHighlight{X,Y}Offset`·s while selected, the stance
//!   bitmap (`hud_pet<stance>.dbr` at its `bitmapPosition`), then the name
//!   (`portraitName`, which no pet record sets);
//! - the portrait's hit area is the frame's rect or the stance bitmap: a left
//!   click toggles the pet's selection, a right click opens the menu at the
//!   cursor; with a hotbar slot armed (`PlayerHotSlotCtrl::IsAnySlotActive`,
//!   hot slots not disabled) a left or right press on the frame's rect
//!   instead casts the slot's skill on the pet ([`CastOnAlly`]), one on the
//!   stance bitmap only goes through to the world;
//! - the menu (class at `PopupMenu::PopupMenu`, layout `PopupMenu::Layout`, draw
//!   `PopupMenu::Draw`, mouse `PopupMenu::HandleMouseEvent`) lists `tagPortraitAggressive`,
//!   `tagPortraitNormal`, `tagPortraitDefensive`, `tagPortraitDisband`; width =
//!   max(60, widest label) + 10, rows `Test String` line height + 3 apart,
//!   labels centred, the hovered row in `style_defaultwhite_sizen_bright`
//!   (others `style_defaultwhite_sizen`), over `records/ui/generic/
//!   basicbackground.dbr` (the rect grown by `Padding`, its frame around the
//!   inner rect, then the filler stretched over it, all tinted white,
//!   `BasicBackground::Draw`; the filler is 65 % black, blended in gamma
//!   space). A click on a row runs it and closes the menu; a click elsewhere
//!   closes it and goes on to the world.
//! - after the stance bitmap, the pet's buff icons (`Character::
//!   GetActiveStatusSkills`, `gd_game::buff_bar::pet_icons`; each skill's up
//!   bitmap, those with a texture only) at 0.4 of their size, white, at
//!   (`PetBuffIconLocationX` + n·`PetBuffIconSeparation`, `PetBuffIconLocationY`)·s
//!   (party members use `BuffIconLocation*`); no timer or tooltip. One row, no
//!   wrapping. PORT: the engine's fixed `panelDistance` step lets the next portrait
//!   cover icons reaching past it; here that portrait moves down by the excess.
//!
//! The icon size is the bitmap's texture size × the UI scale × 0.4
//! (`UIBitmap::SetTexture`).
//!
//! PORT: the menu constants (60, 10, 3, padding) are scaled by the UI scale;
//! key actions 13 / 15 (stance back / forward) and 14 (disband) of
//! `PlayerPortrait::HandleKeyEvent` are not bound; a portrait shows its summon skill's current
//! stance ([`PetStances::portrait`]; vanilla keeps the value each portrait
//! was created with until its own menu changes it). The bar fills are ratio-driven, no pixel
//! quantise; the damage flash runs down on real time.
use super::Ui;
use super::contains;
use super::window::despawn_all;
use crate::state::inventory::UiCapture;
use crate::state::pets::PetSelection;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::collections::HashMap;
use gd_formats::ui::{BarGraph, Rect as URect};
use gd_game::pets::{DisbandPet, Pet, PetStances, SetPetStance, Stance, portrait_pets};
use gd_game_core::casting::CastOnAlly;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::Player;

pub const RECORD: &str = "records/ui/hud/hud_playerstatusdisplay.dbr";
const BACKGROUND: &str = "records/ui/generic/basicbackground.dbr";
const MENU_STYLE: &str = "records/ui/styles/text/style_defaultwhite_sizen.dbr";
const MENU_STYLE_HOVER: &str = "records/ui/styles/text/style_defaultwhite_sizen_bright.dbr";

/// `PlayerPortrait::OpenPetMenu`.
const MENU: [(&str, Option<Stance>); 4] = [
    ("tagPortraitAggressive", Some(Stance::Aggressive)),
    ("tagPortraitNormal", Some(Stance::Normal)),
    ("tagPortraitDefensive", Some(Stance::Defensive)),
    ("tagPortraitDisband", None),
];

pub struct PetPortraitPlugin;

impl Plugin for PetPortraitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PortraitLayout>()
            .init_resource::<PetMenu>()
            .add_systems(
                Update,
                (
                    clicks
                        .in_set(gd_ui_kit::UiInputSet)
                        .after(super::window::WindowClicks)
                        .before(crate::state::casting::CastInputStage::Cast)
                        .before(crate::state::casting::CastInputStage::Armed)
                        .before(crate::state::pets::PetInputSet),
                    (build, animate_bars).chain().in_set(gd_ui_kit::UiPlaySet),
                )
                    .chain(),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            test_hooks.in_set(gd_ui_kit::UiInputSet).before(clicks),
        );
    }
}

/// The open portrait menu: its pet and top-left corner (physical px).
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct PetMenu(pub Option<(Entity, Vec2)>);

/// Physical px.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct PortraitLayout {
    pub portraits: Vec<PortraitHit>,
    pub menu: Option<(URect, f32)>,
}

/// The frame (`BackgroundImagePet`, the rect a cast with an armed slot needs) and the stance bitmap.
#[derive(Clone, PartialEq)]
pub struct PortraitHit {
    pub pet: Entity,
    pub frame: Option<URect>,
    pub stance: Option<URect>,
}

impl PortraitHit {
    fn contains(&self, p: Vec2) -> bool {
        self.frame
            .iter()
            .chain(self.stance.iter())
            .any(|r| contains(r, p))
    }
}

#[derive(Component)]
struct PortraitRoot;

fn stance_record(s: Stance) -> &'static str {
    match s {
        Stance::Normal => "records/ui/hud/hud_petnormal.dbr",
        Stance::Aggressive => "records/ui/hud/hud_petaggressive.dbr",
        Stance::Defensive => "records/ui/hud/hud_petdefensive.dbr",
    }
}

/// One portrait's state, as drawn; the bar widths are animated in place by [`animate_bars`].
#[derive(Clone, PartialEq)]
struct Shown {
    pet: Entity,
    icon: String,
    stance: Stance,
    selected: bool,
    buffs: Vec<String>,
}

/// A portrait's bar fill: the bar's rect (physical px) and the window's inverse scale factor.
#[derive(Component)]
struct PetBarFill {
    pet: Entity,
    mana: bool,
    bar: BarGraph,
    rect: URect,
    inv: f32,
}

/// A pet portrait's damage flash: the life last seen and the red icon's time left (s).
#[derive(Component)]
struct PortraitFlash {
    life: f32,
    left: f32,
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    time: Res<Time<Real>>,
    players: Query<(Entity, Option<&PetStances>), With<Player>>,
    pets: Query<(Entity, &Pet, &Combatant, Has<Dead>)>,
    selection: Res<PetSelection>,
    menu: Res<PetMenu>,
    roots: Query<Entity, With<PortraitRoot>>,
    mut layout: ResMut<PortraitLayout>,
    mut flashes: Query<&mut PortraitFlash>,
    // The pet records' (StatusIcon, StatusIconRed); cleared with a new session.
    mut icons: Local<HashMap<String, (String, String)>>,
    mut built: Local<Option<(UVec3, Vec<Shown>, PetMenu, Option<usize>)>>,
    values: Res<super::HudValues>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        *built = None;
        icons.clear();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let Some(rec) = data.db.get(RECORD) else {
        return;
    };
    let s = assets.scale(size);
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    let (owner, stances) = players
        .single()
        .map(|(e, s)| (Some(e), s))
        .unwrap_or((None, None));
    let order = owner
        .map(|o| {
            portrait_pets(
                o,
                pets.iter()
                    .filter(|(_, _, _, dead)| !dead)
                    .map(|(e, p, _, _)| (e, p)),
            )
        })
        .unwrap_or_default();
    let dt = time.delta_secs();
    for &e in &order {
        let Ok((_, pet, _, _)) = pets.get(e) else {
            continue;
        };
        if !icons.contains_key(pet.record()) {
            let prec = data.db.get(pet.record());
            let g = |k: &str| prec.and_then(|r| r.str(k)).unwrap_or_default().to_owned();
            icons.insert(
                pet.record().to_owned(),
                (g("StatusIcon"), g("StatusIconRed")),
            );
        }
    }
    let mut shown: Vec<(Entity, &str, Stance, bool, &[String])> = Vec::new();
    for &e in &order {
        let Ok((_, pet, cb, dead)) = pets.get(e) else {
            continue;
        };
        // `StatusIconRed` for 500 ms after a life loss (`PlayerPortrait::Update`).
        let left = match flashes.get_mut(e) {
            Ok(mut fl) => {
                fl.left = if cb.life < fl.life {
                    0.5
                } else {
                    (fl.left - dt).max(0.0)
                };
                fl.life = cb.life;
                fl.left
            }
            Err(_) => {
                commands.entity(e).try_insert(PortraitFlash {
                    life: cb.life,
                    left: 0.0,
                });
                0.0
            }
        };
        let red = dead || cb.life <= 0.0 || left > 0.0;
        let (normal, red_icon) = &icons[pet.record()];
        let buffs = values
            .pet_buffs
            .iter()
            .find(|b| b.0 == e)
            .map(|b| b.1.as_slice())
            .unwrap_or(&[]);
        shown.push((
            e,
            if red { red_icon } else { normal },
            PetStances::portrait(stances, &pet.summon),
            selection.0.contains(&e),
            buffs,
        ));
    }
    let cursor = super::cursor_px(win);
    let hover = match (layout.menu, cursor) {
        (Some((r, row_h)), Some(p)) if contains(&r, p) => Some(((p.y - r.y) / row_h) as usize),
        _ => None,
    };
    let sk = size.extend(assets.scale(size).to_bits());
    let same = built.as_ref().is_some_and(|(s, sh, m, h)| {
        *s == sk
            && *m == *menu
            && *h == hover
            && sh.len() == shown.len()
            && sh
                .iter()
                .zip(&shown)
                .all(|(a, &(e, icon, stance, selected, buffs))| {
                    a.pet == e
                        && a.icon == icon
                        && a.stance == stance
                        && a.selected == selected
                        && a.buffs == buffs
                })
    });
    if size.x == 0 || same {
        return;
    }
    let shown: Vec<Shown> = shown
        .into_iter()
        .map(|(pet, icon, stance, selected, buffs)| Shown {
            pet,
            icon: icon.to_owned(),
            stance,
            selected,
            buffs: buffs.to_vec(),
        })
        .collect();
    *built = Some((sk, shown.clone(), *menu, hover));
    despawn_all(&mut commands, &roots);
    *layout = PortraitLayout::default();
    if shown.is_empty() {
        return;
    }
    let root = gd_ui_kit::spawn_hud_overlay(&mut commands, PortraitRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let x0 = (f("initialPanelPositionX") * s).round();
    let y_off = f("initialPanelPositionY") * s;
    let bottom = rec.str("windowScreenAlignmentY") == Some("Bottom");
    let y0 = if bottom { y_off } else { ui.screen.y - y_off }.round();
    let dist = f("panelDistance") * s;
    let rect = |ui: &Ui, pos: Vec2, k: &str| {
        URect::new(
            pos.x + ui.sc(f(&format!("{k}X"))),
            pos.y + ui.sc(f(&format!("{k}Y"))),
            ui.sc(f(&format!("{k}Width"))),
            ui.sc(f(&format!("{k}Height"))),
        )
    };
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    // PORT: the engine steps `panelDistance` per portrait whatever the buffs
    // (`PlayerStatusPanel::LayoutPortraits`); here an icon row ending below the step pushes the
    // next portrait down by the excess.
    let bar = BarGraph::default();
    let mut next_y = y0;
    for p in shown.iter() {
        let pos = Vec2::new(x0, next_y.round());
        let (hr, mr) = (rect(&ui, pos, "PetHealthBar"), rect(&ui, pos, "PetManaBar"));
        ui.solid(hr, Color::BLACK);
        ui.solid(mr, Color::BLACK);
        let Ok((_, pet, cb, _)) = pets.get(p.pet) else {
            continue;
        };
        let frac = (cb.life / cb.profile.sheet.life.max(1.0)).clamp(0.0, 1.0);
        // `CurrentMana / GetAttribute(5)`, 0 when either is 0 (`PlayerPortrait::Update`).
        let mana = if cb.profile.sheet.energy > 0.0 {
            (pet.energy / cb.profile.sheet.energy).clamp(0.0, 1.0)
        } else {
            0.0
        };
        for (r, frac, color, is_mana) in [
            (hr, frac, Color::srgb(1.0, 0.0, 0.0), false),
            (mr, mana, Color::srgb(0.0, 0.64, 0.28), true),
        ] {
            let e = ui.solid(bar.fill_dst(r, frac), color);
            ui.commands.entity(e).insert(PetBarFill {
                pet: p.pet,
                mana: is_mana,
                bar: bar.clone(),
                rect: r,
                inv: 1.0 / win.scale_factor(),
            });
        }
        if let Some((img, sz)) = ui.image(&p.icon) {
            let r = URect::new(
                pos.x + ui.sc(3.0),
                pos.y + ui.sc(3.0),
                ui.sc(sz.x as f32),
                ui.sc(sz.y as f32),
            );
            ui.quad(img, r, None, Color::WHITE);
        }
        let mut hit = PortraitHit {
            pet: p.pet,
            frame: ui
                .bitmap(&field("BackgroundImagePet"), 0.0, 0.0, pos)
                .map(|(_, r)| r),
            stance: None,
        };
        if p.selected {
            ui.bitmap(
                &field("BackgroundImagePetHighlight"),
                f("BackgroundImagePetHighlightXOffset"),
                f("BackgroundImagePetHighlightYOffset"),
                pos,
            );
        }
        if let Some(b) = ui.record(stance_record(p.stance)) {
            let path = b.str("bitmapName").unwrap_or("").to_string();
            hit.stance = ui
                .bitmap(
                    &path,
                    b.f32("bitmapPositionX").unwrap_or(0.0),
                    b.f32("bitmapPositionY").unwrap_or(0.0),
                    pos,
                )
                .map(|(_, r)| r);
        }
        let (bx, by, step) = (
            ui.sc(f("PetBuffIconLocationX")),
            ui.sc(f("PetBuffIconLocationY")),
            ui.sc(f("PetBuffIconSeparation")),
        );
        let (mut n, mut row_h) = (0.0, 0.0f32);
        for b in &p.buffs {
            let Some((img, sz)) = ui.image(b) else {
                continue;
            };
            let r = URect::new(
                pos.x + bx + n * step,
                pos.y + by,
                ui.sc(sz.x as f32) * 0.4,
                ui.sc(sz.y as f32) * 0.4,
            );
            ui.quad_stretch(img, r, Color::WHITE);
            row_h = row_h.max(r.h);
            n += 1.0;
        }
        next_y += dist + (by + row_h - dist).max(0.0);
        layout.portraits.push(hit);
    }
    if let PetMenu(Some((_, at))) = *menu {
        let labels: Vec<String> = MENU
            .iter()
            .map(|(t, _)| ui.fonts.tag(t).to_string())
            .collect();
        let row_h = ui.line_height(MENU_STYLE) + ui.sc(3.0);
        let min_w = ui.sc(60.0);
        let widest = labels
            .iter()
            .map(|l| ui.text_width(MENU_STYLE, l))
            .fold(min_w, f32::max);
        let w = (widest + ui.sc(10.0)).round();
        let r = URect::new(
            at.x.round(),
            at.y.round(),
            w,
            (row_h * labels.len() as f32).round(),
        );
        background(&mut ui, r);
        for (i, l) in labels.iter().enumerate() {
            let style = if hover == Some(i) {
                MENU_STYLE_HOVER
            } else {
                MENU_STYLE
            };
            let x = (r.x + (w - ui.text_width(style, l)) * 0.5).trunc();
            ui.text_line(
                style,
                Vec2::new(x, (r.y + i as f32 * row_h).trunc()),
                l,
                1.0,
            );
        }
        layout.menu = Some((r, row_h));
    }
}

/// The portrait bar fills in place, from the pet's current life / energy ratio.
fn animate_bars(
    pets: Query<(&Pet, &Combatant)>,
    mut fills: Query<(&PetBarFill, &mut Node, &mut Visibility)>,
) {
    for (f, mut node, mut vis) in &mut fills {
        let Ok((pet, cb)) = pets.get(f.pet) else {
            continue;
        };
        let frac = if f.mana {
            if cb.profile.sheet.energy > 0.0 {
                (pet.energy / cb.profile.sheet.energy).clamp(0.0, 1.0)
            } else {
                0.0
            }
        } else {
            (cb.life / cb.profile.sheet.life.max(1.0)).clamp(0.0, 1.0)
        };
        vis.set_if_neq(if frac > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        super::set_node_rect(&mut node, f.bar.fill_dst(f.rect, frac), f.inv);
    }
}

/// `basicbackground.dbr` (`BasicBackground::Draw`): the rect grown by `Padding`, the frame around its
/// inner rect, then the filler over it. Black pieces, shade in alpha (filler 166 / 255), white-tinted.
fn background(ui: &mut Ui, r: URect) {
    let Some(rec) = ui.record(BACKGROUND) else {
        return;
    };
    let pad = ui.sc(rec.f32("Padding").unwrap_or(0.0));
    let outer = URect::new(r.x - pad, r.y - pad, r.w + 2.0 * pad, r.h + 2.0 * pad);
    let path = |k: &str| rec.str(k).unwrap_or("");
    let paths = [
        path("ULBitmap"),
        path("URBitmap"),
        path("BLBitmap"),
        path("BRBitmap"),
        path("TopBitmap"),
        path("BottomBitmap"),
        path("LeftBitmap"),
        path("RightBitmap"),
    ];
    // border_paths lays its pieces outside the rect it is given: the inner rect of the grown one.
    let mut size = |p: &str| {
        ui.image(p)
            .map(|(_, sz)| Vec2::new(ui.sc(sz.x as f32), ui.sc(sz.y as f32)))
            .unwrap_or(Vec2::ZERO)
    };
    let (ul, ur, bl) = (size(paths[0]), size(paths[1]), size(paths[2]));
    let inner = URect::new(
        outer.x + ul.x,
        outer.y + ul.y,
        outer.w - ul.x - ur.x,
        outer.h - ul.y - bl.y,
    );
    ui.border_paths(&paths, inner, 1.0);
    if let Some((h, _)) = ui.image(path("FillerBitmap"))
        && inner.w > 0.0
        && inner.h > 0.0
    {
        ui.quad_stretch(h, inner, Color::WHITE);
    }
}

#[cfg(feature = "dev")]
fn test_hooks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    layout: Res<PortraitLayout>,
    mut menu: ResMut<PetMenu>,
    mut selection: ResMut<PetSelection>,
    mut script: Local<Option<(Option<f32>, Option<f32>)>>,
) {
    let timers = script.get_or_insert_with(|| (hooks.ui.pet_select, hooks.ui.pet_menu));
    let Some(hit) = layout.portraits.first() else {
        return;
    };
    if timers.0.is_some_and(|t| time.elapsed_secs() >= t) {
        timers.0 = None;
        info!("pets test: selecting the first portrait");
        selection.toggle(hit.pet);
    }
    if timers.1.is_some_and(|t| time.elapsed_secs() >= t)
        && let Some(r) = hit.frame.or(hit.stance)
    {
        timers.1 = None;
        info!("pets test: opening the first portrait's menu");
        menu.0 = Some((hit.pet, Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5)));
    }
}

/// Portrait and menu clicks; the UI takes the click.
#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    armed: Res<gd_game::hotbar::ArmedSlot>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<PortraitLayout>,
    mut menu: ResMut<PetMenu>,
    mut selection: ResMut<PetSelection>,
    mut capture: ResMut<UiCapture>,
    mut stance: MessageWriter<SetPetStance>,
    mut disband: MessageWriter<DisbandPet>,
    mut cast: MessageWriter<CastOnAlly>,
) {
    if let Some((pet, _)) = menu.0
        && !layout.portraits.iter().any(|h| h.pet == pet)
    {
        menu.0 = None;
    }
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(win) else {
        return;
    };
    let pressed =
        buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right);
    if let (Some((pet, _)), Some((r, row_h))) = (menu.0, layout.menu) {
        if contains(&r, p) {
            capture.0 = true;
            if pressed {
                let row = ((p.y - r.y) / row_h) as usize;
                match MENU.get(row) {
                    Some((_, Some(s))) => {
                        stance.write(SetPetStance { pet, stance: *s });
                    }
                    Some((_, None)) => {
                        disband.write(DisbandPet { pet });
                    }
                    None => {}
                }
                menu.0 = None;
            }
            return;
        }
        if pressed {
            menu.0 = None;
        }
    }
    if capture.0 {
        return;
    }
    let Some(hit) = layout.portraits.iter().find(|h| h.contains(p)) else {
        return;
    };
    // A slot armed: a press on the frame casts its skill on the pet
    // (`PlayerPortrait::HandleMouseEvent`); one on the stance bitmap only goes on to the world.
    if pressed && armed.0.is_some() {
        if hit.frame.is_some_and(|r| contains(&r, p)) {
            capture.0 = true;
            cast.write(CastOnAlly { ally: hit.pet });
        }
        return;
    }
    capture.0 = true;
    if buttons.just_pressed(MouseButton::Left) {
        selection.toggle(hit.pet);
    } else if buttons.just_pressed(MouseButton::Right) {
        menu.0 = Some((hit.pet, p));
    }
}
