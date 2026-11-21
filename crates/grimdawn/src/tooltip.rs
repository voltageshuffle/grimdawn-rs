//! Item tooltips: hovering an item in a bag, an equipment slot, a vendor
//! grid or a ground label shows the game's item tooltip window.
//!
//! Text: `gd_sim::items::tooltip_item::item_tooltip` (line order, wording and
//! text classes of every item class). Each line's class picks a font style
//! and indent from `records/game/gameengine.dbr` (`GameEngine::
//! GetGameTextStyleName` / `GetGameTextStyleIndent`: the engine
//! keeps a class -> style-record map loaded from that record's keys, see
//! `gd_sim::items::tooltip::CLASS_KEYS`; cached in `ui::text::FontCache::class_style`, shared with `ui::tip`).
//! Window: the `RolloverStyleUIHelp` record the exe selects
//! (`style_itemsequipped` for an equipped item, else `style_itemsinventory`,
//! exe 0x140237370; `style_itemsground` for the ground labels'
//! rollover): background colour + alpha, eight border bitmaps and
//! `minCharsBeforeWrapping`. Line pads, icons, the wrap width and the window size: [`layout`],
//! [`spawn_box`]; PORT: the placement (beside the cursor, kept on screen).
//!
//! Delay (the rollover window, `ui::tip` module doc): the window shows once the item has been hovered its
//! style's `delay` ms (250 for `style_itemsinventory` / `style_itemsequipped`; `style_itemsground` has none), at once
//! while game time is paused; another item restarts the count. Under the game menu nothing is hovered
//! (`ui::rollover` module doc). The counter is the shared [`crate::ui::tip::TipDelay`] (real time, the same rules).
//!
//! Ground labels: `sub_14028AE50` (the ground label manager's update): the hovered label's tooltip opens when
//! the option `autoItemTooltips` is on or the key action 36 ("Display World Item Tooltips", `keys::GROUND_TOOLTIP`, X by
//! default) is held; the hovered label is `loot::HoveredLabel` (its drawn rect). (The engine's action numbers are `keys.rs`'s: 35 / 36 / 37 are Alt / X / Z.)
//!
//! Comparison (GrimDawn.exe item rollover build `sub_1401EE9F0`,
//! compare list `sub_1401E3760` / `sub_1401BD5B0`): an item hovered outside
//! the equipment (bags, stash, vendor, ground) is compared with every worn
//! item of the same `Item_Type` (`gd_sim::items::illusion::item_type`; both
//! rings, both weapons), each in its own "Currently Equipped" box
//! (`tagItemCompareEquipped`, equipped style). When there is one, the hovered
//! item's tooltip starts with the owner widget's header line (class 2
//! `ItemBanner`): `tagItemCompareInventory` (bags, stash; the rollover's
//! default), `tagItemCompareVendor` or `tagItemCompareGround` (the ground
//! items widget `sub_14028A4F0`). PORT: the boxes sit side by side, tops
//! aligned, 16 px apart, the compared ones first (as in the real game);
//! the group goes right of the cursor when it fits,
//! else left.
use crate::data::GameData;
use crate::inventory::{InvLayout, Inventory, SLOTS};
use crate::loot::GroundItem;
use crate::player::Player;
use crate::ui::contains;
use crate::ui::text::{FontCache, LoadedFont, active_color, bitmap_width, wrap_by};
use crate::ui::{HudValues, UiPanels};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::item::Item;
use gd_formats::ui::layout_text;
use gd_render::textures::Textures;
use gd_sim::items::tooltip::{Have, Line, LineKind, tc};
use gd_sim::items::tooltip_item::{TipCtx, item_tooltip};
use std::borrow::Cow;
use std::collections::HashMap;

pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            hover
                .after(crate::player::PlayerSet)
                .after(crate::ui::HudSet)
                .in_set(crate::ui::UiPlaySet),
        );
    }
}

#[derive(Component)]
struct TooltipBox {
    /// x of this box in its group and the group's size (logical px).
    dx: f32,
    group: Vec2,
}

/// Where the hovered item is (picks the rollover style and header).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spot {
    Equipped,
    Inventory,
    Ground,
}

impl Spot {
    /// The header tag of a compared item (`priced`: a vendor's).
    fn header(self, priced: bool) -> Option<&'static str> {
        match self {
            Spot::Equipped => None,
            Spot::Ground => Some("tagItemCompareGround"),
            Spot::Inventory if priced => Some("tagItemCompareVendor"),
            Spot::Inventory => Some("tagItemCompareInventory"),
        }
    }

    /// The rollover style of its window (module doc).
    fn style(self) -> &'static str {
        match self {
            Spot::Equipped => "records/ui/styles/rollover/style_itemsequipped.dbr",
            Spot::Ground => "records/ui/styles/rollover/style_itemsground.dbr",
            Spot::Inventory => "records/ui/styles/rollover/style_itemsinventory.dbr",
        }
    }
}

#[derive(SystemParam)]
struct Gfx<'w> {
    data: Res<'w, GameData>,
    tex: ResMut<'w, Textures>,
    images: ResMut<'w, Assets<Image>>,
    font_assets: ResMut<'w, Assets<Font>>,
    hud: Res<'w, HudValues>,
    panels: Res<'w, UiPanels>,
    trade: Res<'w, crate::trade::Trade>,
    shrine: Res<'w, crate::ui::shrine::ShrineLayout>,
    rules: Option<Res<'w, crate::combat::CombatRules>>,
    loaded: Option<Res<'w, crate::session::LoadedCharacter>>,
    pick: Option<Res<'w, crate::ui::hotbar_pick::PickLayout>>,
    quest: Res<'w, crate::ui::questpopup::QuestPopup>,
    pause: Res<'w, gd_game::pause::GamePause>,
    time: Res<'w, Time<Real>>,
    scale: Res<'w, crate::settings::UiScaleOption>,
    stack: Res<'w, crate::ui::stack::StackWindow>,
    confirm: Res<'w, crate::ui::apply::InvConfirm>,
}

/// One rasterised line: image and its position in the content box (px).
struct Placed {
    image: Handle<Image>,
    pos: Vec2,
    size: Vec2,
}

/// The font style of a record; a style without `fontColor1` has one colour
/// (the text is not a top-to-bottom gradient to white).
fn line_style(gfx: &Gfx, fonts: &mut FontCache, path: &str) -> Option<gd_formats::ui::FontStyle> {
    let mut st = fonts.style(&gfx.data, path)?;
    if gfx
        .data
        .db
        .get(path)
        .is_some_and(|r| r.f32("fontColor1.R").is_none())
    {
        st.color1 = st.color0;
    }
    Some(st)
}

/// The rollover's wrap sample (exe `sub_1401EE9F0`): the item tooltip request has no size of its
/// own (`Item` vtable +1152 `GetRolloverSize` is a no-op but for notes), so the window measures this text
/// (mode 1 of `sub_140236020`): 40 characters below a 1600 px wide screen, else 54.
fn wrap_sample(screen_w: u32) -> &'static str {
    if screen_w < 1600 {
        "The quick brown fox jumped over the lazy"
    } else {
        "The quick brown fox jumped over the lazy dog The quick"
    }
}

/// Lays out and rasterises all lines; returns the placed images and the content size (px).
///
/// Exe `sub_140236020` layout, `sub_14023FB90` draw; Game.dll `GameTextString` ctor
/// @0x180316b90 / `sub_180316CE0`: each line is a `GameTextString` with a left pad `(indent + 6) x UI
/// scale` (the class's `GetGameTextStyleIndent`), a right pad `6 x scale`, and, with a bitmap, the icon
/// (`GetIconWidth`: texture size x line scale x UI scale) between the left pad and the text plus one more
/// right pad; its height is the text's (or the icon's when taller). The wrap width is the request's sample
/// ([`wrap_sample`]) as a `GameTextString` of the request's style, which the item rollover leaves empty:
/// `StyleManager::LoadStyle` of an unknown record gives the defaults `fonts/jura.fnt` at 16 (scaled, at
/// least `lowResMinimumFontSize` 8), with the two pads. A line wider than that is wrapped to the width
/// less its pads (and its icon; the icon stays on the first row); the content is as wide as the widest
/// line (the sample itself does not count) and as tall as all lines; text and icon are drawn at the
/// line's top. (`minCharsBeforeWrapping` of the rollover style is loaded, `sub_140237E70`, but this path
/// never reads it.)
fn layout(
    gfx: &mut Gfx,
    fonts: &mut FontCache,
    lines: &[Line],
    s: f32,
    screen_w: u32,
) -> (Vec<Placed>, Vec2) {
    let pad = |px: f32| (px * s) as i32 as f32;
    let right = pad(6.0);
    let default_style = gd_formats::ui::FontStyle {
        font: "fonts/jura.fnt".into(),
        size: 16,
        min_size: 8,
        color0: [1.0; 3],
        color1: [1.0; 3],
        flags: 0,
    };
    let mut sample_w = 0.0;
    if let Some(f) = fonts.font(&gfx.data, &default_style.font, &mut gfx.font_assets)
        && let LoadedFont::Bitmap { font, .. } = &*f
    {
        sample_w = layout_text(
            font,
            default_style.scaled_size(s),
            0,
            wrap_sample(screen_w),
            (default_style.color0, default_style.color1),
        )
        .width as f32;
    }
    let max_total = if sample_w > 0.0 {
        sample_w + pad(6.0) + right
    } else {
        (360.0 * s).round()
    };
    debug!("tooltip wrap: sample {sample_w} px, line limit {max_total} px (scale {s})");
    let mut out = Vec::new();
    let (mut y, mut width) = (0.0f32, 0.0f32);
    for l in lines {
        let (Some(path), indent) = fonts.class_style(&gfx.data, l.class) else {
            continue;
        };
        let Some(st) = line_style(gfx, fonts, &path) else {
            continue;
        };
        let Some(f) = fonts.font(&gfx.data, &st.font, &mut gfx.font_assets) else {
            continue;
        };
        let LoadedFont::Bitmap { font, atlas } = &*f else {
            continue;
        };
        let size = st.scaled_size(s);
        let left = pad(indent + 6.0);
        let icon = l.icon.as_ref().and_then(|(path, scale)| {
            let h = gfx.tex.get(&gfx.data, path, true, &mut gfx.images)?;
            let sz = gfx.images.get(&h)?.size();
            let px = |v: u32| (v as f32 * scale * s) as i32 as f32;
            Some((h, Vec2::new(px(sz.x), px(sz.y))))
        });
        let line_h = layout_text(font, size, st.flags, "A", (st.color0, st.color1)).height as f32;
        if l.text.is_empty() && icon.is_none() {
            // An empty `GameTextString` is one line of its style.
            y += line_h;
            width = width.max(left + right);
            continue;
        }
        let text = l.text.replace("^n", "\n").replace("^N", "\n");
        // The text room of a wrapped line: the sample width less this line's pads (and icon).
        let room = |with_icon: bool| {
            let icon_w = icon.as_ref().filter(|_| with_icon).map_or(0.0, |i| i.1.x);
            if with_icon && icon.is_some() {
                max_total - left - icon_w
            } else {
                max_total - left - right
            }
        };
        // The colour in effect carries over a `^n` break (real window: the `^w`
        // before `^n(Used in amulets and medals)` makes that line white).
        let mut carry = String::new();
        let mut first = true;
        for part in text.split('\n') {
            let part_c = format!("{carry}{part}");
            carry = active_color(&part_c);
            let max_w = room(first).trunc();
            for row in wrap_by(&part_c, max_w, |t| bitmap_width(font, size, st.flags, t)) {
                let row_icon = if first { icon.clone() } else { None };
                first = false;
                let mut x = left;
                let mut h = line_h;
                if let Some((img, isz)) = row_icon {
                    out.push(Placed {
                        image: img,
                        pos: Vec2::new(x, y),
                        size: isz,
                    });
                    x += isz.x + right;
                    h = h.max(isz.y);
                }
                match crate::ui::text::rasterize(font, atlas, &st, size, &row) {
                    Some(ti) => {
                        width = width.max(x + ti.layout.width as f32 + right);
                        h = h.max(ti.layout.height as f32);
                        out.push(Placed {
                            image: gfx.images.add(ti.image),
                            pos: Vec2::new(x + ti.offset.x, y + ti.offset.y),
                            size: Vec2::new(ti.size.x as f32, ti.size.y as f32),
                        });
                    }
                    None => width = width.max(x + right),
                }
                y += h;
            }
        }
    }
    (out, Vec2::new(width, y))
}

/// The weapon-compare DPS of hovered items: finished values (per item and Ctrl state) and the running job.
/// Results last until the inventory or the level changes (`sig`).
#[derive(Default)]
struct DpsJobs {
    sig: (u64, i32),
    done: HashMap<(Item, bool), (Option<f32>, Option<f32>)>,
    job: Option<((Item, bool), bevy::tasks::Task<(Option<f32>, Option<f32>)>)>,
}

/// What is hovered: the item (borrowed unless built for this frame), where, and a vendor price.
type Hovered<'a> = Option<(Cow<'a, Item>, Spot, Option<u64>)>;

fn same(a: &Hovered, b: &Option<(Item, Spot, Option<u64>)>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some((i, s, p)), Some((j, t, q))) => s == t && p == q && **i == *j,
        _ => false,
    }
}

fn owned(h: &Hovered) -> Option<(Item, Spot, Option<u64>)> {
    h.as_ref().map(|(i, s, p)| ((**i).clone(), *s, *p))
}

#[allow(clippy::too_many_arguments)]
fn hover(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    hovered_label: Res<crate::loot::HoveredLabel>,
    layout_res: Res<InvLayout>,
    inv: Option<Res<Inventory>>,
    ground: Query<&GroundItem, Without<crate::loot::FilteredOut>>,
    fonts: ResMut<FontCache>,
    progress: Option<Res<crate::loot::PlayerProgress>>,
    players: Query<
        (
            &crate::combat::Combatant,
            &gd_game::player::Loadout,
            &gd_game::casting::Caster,
            Option<&gd_game::weapon_pool::SkillLinks>,
            &gd_game::character::Character,
        ),
        With<Player>,
    >,
    boxes: Query<(Entity, &TooltipBox)>,
    mut nodes: Query<(&mut Node, &mut Visibility), With<TooltipBox>>,
    mut shown: Local<Option<(Item, Spot, Option<u64>)>>,
    mut gfx: Gfx,
    (
        keys,
        options,
        mut last_details,
        mut fresh,
        hooks,
        mut dps,
        bindings,
        stash,
        anims,
        mut waited,
        mut shown_version,
    ): (
        Res<ButtonInput<KeyCode>>,
        Res<gd_game::options::GameOptions>,
        Local<bool>,
        gd_game::session::NewSession,
        Res<gd_game::hooks::Hooks>,
        Local<DpsJobs>,
        Res<crate::ui::keys::KeyBindings>,
        Option<Res<gd_game::stash::Stash>>,
        Res<gd_game::character::AnimCache>,
        // The hovered item and its show delay (module doc, "Delay"; the shared `ui::tip::TipDelay`).
        Local<(Option<(Item, Spot, Option<u64>)>, crate::ui::tip::TipDelay)>,
        // `Inventory::version` the shown box was built at: a weapon swap or any equipment change rebuilds
        // the compare box and DPS change.
        Local<u64>,
    ),
) {
    if fresh.begun() {
        *dps = Default::default();
        *shown = Default::default();
        *waited = (None, crate::ui::tip::TipDelay::skill(String::new(), 0.0));
    }
    let (Ok(win), Some(inv), mut fonts) = (window.single(), inv, fonts) else {
        return;
    };
    let anim_frames = anims.shared();
    // `GD_HOVER` (hooks.rs) stands in for the mouse.
    let cursor = crate::ui::cursor_px(&hooks, win).map(|p| p / win.scale_factor());
    // What is under the cursor.
    let mut hovered: Hovered = None;
    // `GD_HOVER_ITEM` (hooks.rs; with `GD_HOVER`, optionally `GD_HOVER_PRICE`): shows that item's tooltip.
    if let Some(v) = hooks.items.hover_item.clone() {
        let mut it = v.split(',').map(str::to_string);
        let mut n = || it.next().unwrap_or_default();
        let (base_name, relic_name, prefix_name, suffix_name) = (n(), n(), n(), n());
        let item = Item {
            base_name,
            relic_name,
            prefix_name,
            suffix_name,
            seed: 12345,
            var1: 1,
            stack_count: 1,
            ..Default::default()
        };
        let price = hooks.items.hover_price;

        // `GD_HOVER_SPOT` (default: a bag item).
        let spot = match hooks.items.hover_spot.clone().as_deref() {
            Some("ground") => Spot::Ground,
            Some("equipped") => Spot::Equipped,
            _ => Spot::Inventory,
        };
        hovered = Some((Cow::Owned(item), spot, price));
    } else if let Some(c) = cursor
        && inv.held.is_none()
    {
        let p = c * win.scale_factor();
        if gfx.panels.character && layout_res.window.is_some_and(|w| contains(&w, p)) {
            for (b, r, cell) in &layout_res.grids {
                if contains(r, p) {
                    let (cx, cy) = (((p.x - r.x) / cell) as u32, ((p.y - r.y) / cell) as u32);
                    hovered = inv.bags[*b]
                        .item_at(cx, cy)
                        .map(|x| (Cow::Borrowed(&x.item), Spot::Inventory, None));
                }
            }
            for (slot, r) in &layout_res.slots {
                if contains(r, p) {
                    hovered = inv.equipped[SLOTS.iter().position(|s| s == slot).unwrap()]
                        .as_ref()
                        .map(|i| (Cow::Borrowed(i), Spot::Equipped, None));
                }
            }
        } else if let Some(item) = gfx
            .pick
            .as_ref()
            .and_then(|p| p.scroll.as_deref())
            .and_then(|rec| {
                inv.bags
                    .iter()
                    .flat_map(|b| &b.items)
                    .map(|p| &p.item)
                    .find(|i| i.base_name.eq_ignore_ascii_case(rec))
            })
        {
            // A hot slot pick list's scroll (`GetPotionTextOfType`: the first carried item).
            hovered = Some((Cow::Borrowed(item), Spot::Inventory, None));
        } else if let Some((item, _)) = gfx.quest.hovered_item() {
            // The Quest Complete window's item icon (`ui/questpopup.rs`).
            hovered = Some((Cow::Owned(item), Spot::Inventory, None));
        } else if let Some(item) = &gfx.shrine.hover_item {
            hovered = Some((Cow::Borrowed(item), Spot::Inventory, None));
        } else if let Some((item, price)) = &gfx.trade.hover {
            hovered = Some((Cow::Borrowed(item), Spot::Inventory, *price));
        } else if (options.flag("autoItemTooltips")
            || bindings.pressed(crate::ui::keys::GROUND_TOOLTIP, &keys))
            && let Some(e) = hovered_label.0
        {
            hovered = ground
                .get(e)
                .ok()
                .and_then(|g| g.item.as_ref())
                .map(|i| (Cow::Borrowed(i), Spot::Ground, None));
        }
    }
    // The game menu takes the mouse: nothing under it is hovered (`ui::rollover` module doc); nor under a
    // modal question or the stack window (whose buttons the tooltip would cover).
    if gfx.pause.menu
        || crate::inventory::question_up(&inv, &gfx.trade, &gfx.confirm)
        || cursor.is_some_and(|c| gfx.stack.covers(c * win.scale_factor()))
    {
        hovered = None;
    }
    if !same(&hovered, &waited.0) {
        let delay = hovered
            .as_ref()
            .and_then(|s| gfx.data.db.get(s.1.style()))
            .and_then(|r| r.i32("delay"))
            .unwrap_or(0) as f32;
        *waited = (
            owned(&hovered),
            crate::ui::tip::TipDelay::skill(String::new(), delay),
        );
    }
    let tip_shown = waited.1.tick(gfx.time.delta_secs() * 1000.0, &gfx.pause);
    // The details flag (`sub_140225110`): Ctrl, the left trigger or
    // the option `detailItemTooltips` (bool 68); a change rebuilds the text.
    let details_option = options.flag("detailItemTooltips");
    let scroll_pick = gfx.pick.as_ref().is_some_and(|p| p.scroll.is_some());
    let details = details_option
        || scroll_pick
        || keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !same(&hovered, &shown)
        || (hovered.is_some() && (details != *last_details || inv.version != *shown_version))
    {
        *shown_version = inv.version;
        *last_details = details;
        for (e, _) in &boxes {
            commands.entity(e).try_despawn();
        }
        *shown = owned(&hovered);
        drop(hovered);
        let Some((item, spot, price)) = shown.clone() else {
            return;
        };
        let idb = gfx.data.item_db();
        let sheet = players.single().ok().map(|c| &c.0.profile.sheet);
        let have = Have {
            level: progress.as_ref().map_or(1.0, |p| p.progress.level as f32),
            physique: sheet.as_ref().map_or(0.0, |s| s.physique),
            cunning: sheet.as_ref().map_or(0.0, |s| s.cunning),
            spirit: sheet.as_ref().map_or(0.0, |s| s.spirit),
            reduction: players
                .single()
                .map_or_else(|_| Default::default(), |c| c.0.profile.req_reduction),
        };
        // Set counts read what is worn (the Loadout source), not a detached slot's item.
        let equipped = inv.worn();
        // The skill modifier blocks' numbers go through the player's profile.
        let player = players.single().ok().map(|p| p.0);
        let skill_ctx = gd_sim::skill_tooltip::SkillTipCtx {
            db: &gfx.data.db,
            trees: &gfx.data.skill_trees(),
            tags: &fonts.tags,
            book: &gfx.hud.skills,
            player: gfx
                .rules
                .as_ref()
                .zip(player)
                .map(|(r, c)| (&*r.0, &*c.profile)),
            pet_bonuses: &[],
        };
        // `GameEngine::IsItemInFormulas` (`ItemArtifactFormula::GetUIDisplayText`): the
        // account's learned blueprints (`formulas.gst`).
        let known = |record: &str| {
            stash.as_ref().is_some_and(|s| {
                s.formulas
                    .formula_records()
                    .iter()
                    .any(|l| l.eq_ignore_ascii_case(record))
            })
        };
        // The base attack speed line's animation factor: the player's weapon
        // attack animation (`CharAttributeVal_BaseAttackSpeed::CreateText`).
        let weapon_anim_rate = players.single().ok().map(|(_, _, _, _, character)| {
            gd_game::sheet_dps::weapon_anim_rate(&gfx.data, &character.anims, &anim_frames)
        });
        let ctx = TipCtx {
            db: &idb,
            tags: &fonts.tags,
            have,
            equipped: &equipped,
            suppress_directions: price.is_some(),
            formula_known: Some(&known),
            skills: Some(&skill_ctx),
            details,
            details_option,
            dps_delta: None,
            dps_offhand: None,
            weapon_anim_rate,
        };
        // The worn items it is compared with.
        let ty = |it: &Item| {
            idb.get(&it.base_name)
                .map_or(0, |r| gd_sim::items::illusion::item_type(&r.class))
        };
        let compared: Vec<Item> = match spot {
            Spot::Equipped => Vec::new(),
            _ => inv
                .equipped
                .iter()
                .flatten()
                .filter(|e| ty(e) != 0 && ty(e) == ty(&item) && **e != item)
                .cloned()
                .collect(),
        };
        // `GetUIDisplayText_DPS`: only against a worn weapon, never for the
        // worn copy (`gd_game::sheet_dps::compare_dps`).
        let is_weapon = idb
            .get(&item.base_name)
            .is_some_and(|r| r.class.starts_with("Weapon") && !r.class.starts_with("WeaponArmor"));
        let mut dps_delta = None;
        let mut dps_offhand = None;
        if is_weapon
            && !compared.is_empty()
            && let (Ok((cb, loadout, caster, links, character)), Some(rules)) =
                (players.single(), gfx.rules.as_ref())
        {
            // `compare_dps` rebuilds the player's profile twice (about 75 ms each): it runs on the task pool
            // and the tooltip waits for it (nothing drawn meanwhile); results last until the inventory
            // or the level changes.
            let sig = (inv.version, cb.profile.level);
            if dps.sig != sig {
                dps.sig = sig;
                dps.done.clear();
                dps.job = None;
            }
            let key = (item.clone(), details);
            if let Some(v) = dps.done.get(&key) {
                (dps_delta, dps_offhand) = *v;
            } else {
                if dps.job.as_ref().is_none_or(|(k, _)| *k != key) {
                    let (data, rules, profile) =
                        (gfx.data.clone(), rules.0.clone(), cb.profile.clone());
                    let (loadout, caster, links) =
                        (loadout.clone(), caster.clone(), links.cloned());
                    let (record, anims) = (character.record.clone(), character.anims.clone());
                    let frames = anim_frames.clone();
                    let (now, with) = (inv.weapon_gear(None), inv.weapon_gear(Some(&item)));
                    let off = inv.weapon_gear_offhand(&item);
                    let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                        // `compare_dps` derives each tried gear's effective skills itself.
                        let effective = loadout.effective_skills(&data.db, &data.skill_trees());
                        let inputs = gd_game::sheet_dps::Inputs {
                            data: &data,
                            rules: &rules,
                            profile: &profile,
                            loadout: &loadout,
                            effective: &effective,
                            caster: &caster,
                            links: links.as_ref(),
                            record: &record,
                            anims: &anims,
                        };
                        let main =
                            gd_game::sheet_dps::compare_dps(&inputs, &frames, &now, &with, details);
                        // The off-hand compare: the candidate in the off hand against the gear as worn.
                        let off = off.and_then(|g| {
                            gd_game::sheet_dps::compare_dps(&inputs, &frames, &now, &g, details)
                        });
                        (main, off)
                    });
                    dps.job = Some((key.clone(), task));
                }
                let finished = dps.job.as_mut().and_then(|(_, t)| {
                    bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(t))
                });
                match finished {
                    Some(v) => {
                        dps.job = None;
                        dps.done.insert(key, v);
                        (dps_delta, dps_offhand) = v;
                    }
                    None => {
                        // Not ready: ask again next frame (`shown` no longer matches `hovered`).
                        *shown = None;
                        return;
                    }
                }
            }
        }
        let ctx_dps = TipCtx {
            dps_delta,
            dps_offhand,
            ..ctx
        };
        let mut lines = item_tooltip(&ctx_dps, &item);
        if lines.is_empty() {
            return;
        }
        // A merchant's price: `GameEngine::CreateUIPlayerBuyText` @0x1802f5590
        // -> `Market::` @0x18036e1e0 (class 2): `MarketCostAffordable`, or
        // `MarketCostTooExpensive` when the player lacks the iron.
        if let Some(price) = price {
            let key = if price <= gfx.hud.gold {
                "MarketCostAffordable"
            } else {
                "MarketCostTooExpensive"
            };
            if let Some(f) = fonts.tags.get(key) {
                let text = gd_sim::items::tooltip::format_rich(
                    f,
                    &[gd_sim::items::tooltip::Arg::Num(price as f32)],
                );
                // The tags read `Cost  {%d0}` (two spaces); `format` collapses runs.
                let text = text.replacen("Cost ", "Cost  ", 1);
                lines.insert(
                    0,
                    Line {
                        text,
                        kind: LineKind::Base,
                        class: tc::ITEM_BANNER,
                        icon: None,
                    },
                );
            }
        }
        // A bag item while a merchant is open: `GameEngine::CreateUIPlayerSellText`
        // -> `sub_18036E4C0` (class 2): `MarketValue` ("Sell Value  N"), or
        // `CanNotBeSoldHere` at 0 (PORT: the buyback variant
        // `MarketValuePreviousTransaction` is not needed: buyback items are the
        // merchant's and show their cost).
        if price.is_none()
            && spot == Spot::Inventory
            && let Some(crate::trade::TradeOpen::Vendor { market, .. }) = &gfx.trade.open
        {
            let difficulty = gfx
                .loaded
                .as_ref()
                .map(|c| c.0.info.difficulty())
                .unwrap_or(0);
            let discount = gfx
                .trade
                .faction
                .as_ref()
                .map_or(0, gd_game::trade::FactionView::discount);
            let sell = gd_sim::items::market::prices(&idb, market, &item, difficulty, discount)
                .map_or(0, |p| p.1);
            let text = if sell > 0 {
                fonts.tags.get("MarketValue").map(|f| {
                    gd_sim::items::tooltip::format_rich(
                        f,
                        &[gd_sim::items::tooltip::Arg::Num(sell as f32)],
                    )
                    .replacen("Value ", "Value  ", 1)
                })
            } else {
                fonts.tags.get("CanNotBeSoldHere").map(str::to_string)
            };
            if let Some(text) = text {
                lines.insert(
                    0,
                    Line {
                        text,
                        kind: LineKind::Base,
                        class: tc::ITEM_BANNER,
                        icon: None,
                    },
                );
            }
        }
        let banner = |fonts: &FontCache, tag: &str| Line {
            text: fonts.tags.get(tag).unwrap_or(tag).to_string(),
            kind: LineKind::Base,
            class: tc::ITEM_BANNER,
            icon: None,
        };
        if !compared.is_empty()
            && let Some(h) = spot.header(price.is_some())
        {
            lines.insert(0, banner(&fonts, h));
        }
        let mut sets: Vec<(Vec<Line>, Spot)> = compared
            .iter()
            .map(|e| {
                let ctx = TipCtx {
                    suppress_directions: false,
                    ..ctx
                };
                let mut l = item_tooltip(&ctx, e);
                l.insert(0, banner(&fonts, "tagItemCompareEquipped"));
                (l, Spot::Equipped)
            })
            .collect();
        sets.push((lines, spot));
        let boxes: Vec<(Entity, Vec2)> = sets
            .iter()
            .map(|(l, sp)| spawn_box(&mut commands, &mut gfx, &mut fonts, l, *sp, win))
            .collect();
        // Side by side, 16 px apart (physical px at the UI scale).
        let gap = 16.0
            * gfx
                .scale
                .factor(win.physical_width(), win.physical_height())
            / win.scale_factor();
        let group = Vec2::new(
            boxes.iter().map(|b| b.1.x).sum::<f32>() + gap * (boxes.len() - 1) as f32,
            boxes.iter().map(|b| b.1.y).fold(0.0, f32::max),
        );
        let mut dx = 0.0;
        for (e, size) in boxes {
            commands.entity(e).insert(TooltipBox { dx, group });
            dx += size.x + gap;
        }
        return;
    }
    // Follow the cursor, kept inside the window; shown once its style's delay has passed (module doc,
    // "Delay"; the counter is the shared `ui::tip::TipDelay`).
    let Some(c) = cursor else { return };
    if !tip_shown {
        return;
    }
    let (w, h) = (win.width(), win.height());
    for (e, b) in &boxes {
        let Ok((mut node, mut visible)) = nodes.get_mut(e) else {
            continue;
        };
        let size = b.group;
        let x = if c.x + 20.0 + size.x > w {
            (c.x - 12.0 - size.x).max(0.0)
        } else {
            c.x + 20.0
        };
        let y = (c.y + 12.0).min(h - size.y - 10.0).max(0.0);
        node.left = Val::Px(x + b.dx);
        node.top = Val::Px(y);
        visible.set_if_neq(Visibility::Inherited);
    }
}

/// Spawns one tooltip window (hidden until placed); returns it and its size
/// (logical px).
fn spawn_box(
    commands: &mut Commands,
    gfx: &mut Gfx,
    fonts: &mut FontCache,
    lines: &[Line],
    spot: Spot,
    win: &Window,
) -> (Entity, Vec2) {
    let s = gfx
        .scale
        .factor(win.physical_width(), win.physical_height());
    let inv = 1.0 / win.scale_factor();
    let style_path = spot.style();
    let db = gfx.data.db.clone();
    let rec = db.get(style_path);
    let f = |k: &str, d: f32| rec.as_ref().and_then(|r| r.f32(k)).unwrap_or(d);
    let bg = Color::srgba(
        f("backgroundColorRed", 0.03),
        f("backgroundColorGreen", 0.03),
        f("backgroundColorBlue", 0.04),
        f("backgroundColorAlpha", 0.9),
    );
    let (placed, mut content) = layout(gfx, fonts, lines, s, win.physical_width());
    // Border bitmaps (edges are stretched, corners keep their size).
    let mut tex = |key: &str| -> Option<(Handle<Image>, Vec2)> {
        let path = rec.as_ref()?.str(key)?.to_string();
        let h = gfx.tex.get(&gfx.data, &path, true, &mut gfx.images)?;
        let sz = gfx.images.get(&h)?.size();
        Some((h, Vec2::new(sz.x as f32, sz.y as f32)))
    };
    let (top, bottom, left, right) = (
        tex("topTexture"),
        tex("bottomTexture"),
        tex("leftTexture"),
        tex("rightTexture"),
    );
    let (ul, ur, ll, lr) = (
        tex("upperLeftTexture"),
        tex("upperRightTexture"),
        tex("lowerLeftTexture"),
        tex("lowerRightTexture"),
    );
    let th = |t: &Option<(Handle<Image>, Vec2)>, w: bool| {
        t.as_ref()
            .map_or(0.0, |t| (if w { t.1.x } else { t.1.y }) * s)
            .round()
    };
    let (bt, bb, bl, br) = (
        th(&top, false),
        th(&bottom, false),
        th(&left, true),
        th(&right, true),
    );
    debug!(
        "tooltip border px: top {:?} left {:?} ul {:?} (bt {bt} bl {bl})",
        top.as_ref().map(|t| t.1),
        left.as_ref().map(|t| t.1),
        ul.as_ref().map(|t| t.1)
    );
    // The window is its text box (exe `sub_140236A50` size / `sub_140236020` text box: the box
    // plus, from the style's border object, a rect each of the upper-left, left / right and top / bottom
    // bitmaps read through vtable +152 (+8 / +12); those add nothing for the stock rollover styles: the
    // real game's ground-item box is 362 px for a 351 px line, its text 6 px in, the lines' own pads). The
    // border bitmaps are drawn over the box's edges; its height is held to the screen less 10 px.
    content.y = content.y.min(win.physical_height() as f32 - 10.0);
    let (w, h) = (content.x, content.y);
    let node = |x: f32, y: f32, w: f32, h: f32| Node {
        position_type: PositionType::Absolute,
        left: Val::Px(x * inv),
        top: Val::Px(y * inv),
        width: Val::Px(w * inv),
        height: Val::Px(h * inv),
        ..default()
    };
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(w * inv),
                height: Val::Px(h * inv),
                ..default()
            },
            BackgroundColor(bg),
            GlobalZIndex(200),
            Visibility::Hidden,
        ))
        .id();
    let add = |commands: &mut Commands, img: Handle<Image>, x: f32, y: f32, w: f32, h: f32| {
        let e = commands
            .spawn((
                node(x, y, w, h),
                ImageNode::new(img).with_mode(bevy::ui::widget::NodeImageMode::Stretch),
            ))
            .id();
        commands.entity(root).add_child(e);
    };
    // The 1px border correction of the style (`borderCorrectionOffset*`).
    let (cx, cy) = (
        f("borderCorrectionOffsetX", 0.0),
        f("borderCorrectionOffsetY", 0.0),
    );
    let (cw, ch) = (
        f("borderCorrectionOffsetXSize", 0.0),
        f("borderCorrectionOffsetYSize", 0.0),
    );
    let (ex, ey, ew, eh) = (cx.round(), cy.round(), cw.round(), ch.round());
    if let Some((h_, _)) = top {
        add(commands, h_, bl + ex, 0.0, w - bl - br + ew, bt);
    }
    if let Some((h_, _)) = bottom {
        add(commands, h_, bl + ex, h - bb, w - bl - br + ew, bb);
    }
    if let Some((h_, _)) = left {
        add(commands, h_, 0.0, bt + ey, bl, h - bt - bb + eh);
    }
    if let Some((h_, _)) = right {
        add(commands, h_, w - br, bt + ey, br, h - bt - bb + eh);
    }
    for (t, x, y, right_, bottom_) in [
        (&ul, 0.0, 0.0, false, false),
        (&ur, w, 0.0, true, false),
        (&ll, 0.0, h, false, true),
        (&lr, w, h, true, true),
    ] {
        if let Some((h_, sz)) = t {
            let (cw, chh) = ((sz.x * s).round(), (sz.y * s).round());
            add(
                commands,
                h_.clone(),
                if right_ { x - cw } else { x },
                if bottom_ { y - chh } else { y },
                cw,
                chh,
            );
        }
    }
    let (ox, oy) = (0.0, 0.0);
    for p in placed {
        let e = commands
            .spawn((
                node(ox + p.pos.x, oy + p.pos.y, p.size.x, p.size.y),
                ImageNode::new(p.image).with_mode(bevy::ui::widget::NodeImageMode::Stretch),
            ))
            .id();
        commands.entity(root).add_child(e);
    }
    (root, Vec2::new(w, h) * inv)
}
