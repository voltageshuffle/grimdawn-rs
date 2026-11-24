//! Skills window (`records/ui/skills/skills_mastertable.dbr`, opened with `S`):
//! one tab per chosen mastery plus the mastery selection while a slot is free.
//! A mastery pane (`skillCtrlPaneN` → `classtable.dbr`) draws its background,
//! class image, mastery bar (filled by level / 50) and the skill buttons
//! (`tabSkillButtons`: border at `bitmapPosition`, icon at `+skillOffset`,
//! "level / max" in `skillbuttons_skilllevelstyle`, state frames, connector
//! textures, tier lines and numbers) plus the Devotion tab. The undo button,
//! the points line and the reclaim money/price texts belong to the mastery pane
//! only (`skills_classpanelconfiguration.dbr` / `skillpanebase.tpl`), not the
//! selection table (`masterypane.tpl`), so they are drawn only when the active
//! tab has a chosen mastery. Hovering a skill shows its tooltip (`tooltip`, text from
//! `gd_sim::skill_tooltip`). Left click spends a point
//! (`gd_sim::skills` rules). Skills the rules refuse are drawn darkened.
//!
//! Input, `Grim Dawn.exe`: the skill buttons sit in the pane's push button group (`UISkillPane.buttonGroup`, mouse
//! `UIPushButtonGroup::HandleMouseEvent`): only a left press (event type 1) on an enabled button calls the click
//! listener `UISkillPane::OnButtonClick` (spend, or refund in the mode); a right press (type 2) reaches no listener,
//! and the pane's mouse handler `UISkillPane::HandleMouseEvent` only refreshes the hovered button's requirement flags
//! and rollover for both. The button's own handler (`UIButton::HitTest`) is a hit test: nothing is dragged.
//! Keys: the window's key handler `SkillsWindow::HandleKeyEvent` toggles its child window (`SkillsWindow.childWindow`)
//! on key 16 and otherwise forwards to it; the pane's and the buttons' (`UIWidget::HandleKeyEvent`) only forward keys
//! 21 / 22 to their parent. The window never binds a hot slot: `PlayerHotSlotCtrl::SetHotSlot` / `SetPrimarySlot` /
//! `SetSecondarySlot` are called from `HotSlotButton::SetOption` alone, whose callers are the HUD's slot pick list
//! (`ui::hotbar_pick`, `gd_game::hotbar`). Reclamation mode (at a spirit guide, `ui::reclamation`, rules
//! `gd_sim::reclamation`): the reallocation background without the class image, red / green frames, the money and price
//! texts, a click refunds a point and the tooltip ends in the reclaim line.
//!
//! Stacking: the local map and the quest journal can open over this window (their policy rows don't close it)
//! and draw above it, so [`clicks`] and [`tooltip`] ignore a cursor inside either ([`Above`]).
//!
//! Sounds: the close, tab, devotion, undo, mastery and skill buttons register their `soundNameDown` (`ui/widgets.rs`).
use super::contains;
use super::{HudValues, Ui};
use crate::state::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextBox, Widget, WindowDef};
use gd_sim::skills::{class_trees, masteries_allowed, tree_skills};

pub const MASTER: &str = "records/ui/skills/skills_mastertable.dbr";
const BASE_PANE: &str = "records/ui/skills/classcommon/skills_classpanelconfiguration.dbr";

#[derive(Clone, Debug, PartialEq)]
pub enum SkillHit {
    Tab(u8),
    /// The Devotion tab between the mastery tabs.
    Devotion,
    Raise {
        tree: String,
        skill: String,
    },
    /// `undoButton`: revert the points spent since the window opened.
    Undo,
    Close,
}

/// Screen rectangles of the open skills window (physical px).
#[derive(Resource, Default, Clone)]
pub struct SkillLayout {
    pub window: Option<URect>,
    pub hits: Vec<(SkillHit, URect)>,
}

/// Registers the button record at `rec`'s press sound (`soundNameDown`, `ui/widgets.rs`) for its rect `r`.
fn button_sound(ui: &mut Ui, r: URect, rec: &str) {
    if let Some(b) = ui.record(rec) {
        ui.record_button_sound(r, &b);
    }
}

/// (class number, tree) of the chosen masteries, in tree order.
fn chosen(ui: &Ui, values: &HudValues) -> Vec<(u32, String)> {
    ui.data
        .skill_trees()
        .learned(&values.skills)
        .map(|t| (t.class, t.path.clone()))
        .collect()
}

pub fn build(ui: &mut Ui, values: &HudValues, tab: u8) {
    let Some(m) = ui.record(MASTER) else { return };
    let field = |k: &str| m.str(k).map(str::to_string).unwrap_or_default();
    let w = WindowDef::from_record(&m).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let o = Vec2::new(w.x, w.y);
    ui.sound_blocker(w);
    let mut layout = SkillLayout {
        window: Some(w),
        hits: Vec::new(),
    };
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&field("skillsBaseBitmap")) {
        ui.bitmap(&bitmap, x, y, o);
    }
    if let Some(r) = super::window::close_button(ui, &field("skillsCloseButton"), o) {
        layout.hits.push((SkillHit::Close, r));
    }
    let masteries = chosen(ui, values);
    let can_select = masteries.len() < 2;
    let ntabs = masteries.len() + can_select as usize;
    let tab = (tab.max(1) as usize).min(ntabs.max(1));
    for i in 1..=ntabs {
        let Some(Widget::Button { up, down, x, y, .. }) =
            ui.widget(&field(&format!("skillsTab{i}Button")))
        else {
            continue;
        };
        let bmp = if i == tab { down.or(up) } else { up };
        if let Some(b) = bmp
            && let Some((_, r)) = ui.bitmap(&b, x, y, o)
        {
            button_sound(ui, r, &field(&format!("skillsTab{i}Button")));
            layout.hits.push((SkillHit::Tab(i as u8), r));
            let title = match masteries.get(i - 1) {
                Some((n, _)) => ui
                    .record(&field(&format!("skillCtrlPane{n}")))
                    .and_then(|t| t.str("skillTabTitle").map(str::to_string))
                    .map(|t| ui.fonts.tag(&t).to_string())
                    .unwrap_or_default(),
                None => ui.fonts.tag("tagSkillClassSelect").to_string(),
            };
            let tb = TextBox {
                style: None,
                rect: URect::new(x, y, r.w / ui.s, r.h / ui.s),
                align_x: gd_formats::ui::TextAlign::Center,
                align_y: gd_formats::ui::TextAlign::Center,
                tag: None,
                glow: None,
            };
            ui.text_with_style(
                &tb,
                "records/ui/styles/text/style_special_classtabs.dbr",
                o,
                &title,
            );
        }
    }
    // Middle tab (`skillsTab3Button`, the devotion graphic): opens the devotion window.
    if let Some(Widget::Button { up, down, x, y, .. }) = ui.widget(&field("skillsTab3Button"))
        && let Some(b) = up.or(down)
        && let Some((_, r)) = ui.bitmap(&b, x, y, o)
    {
        button_sound(ui, r, &field("skillsTab3Button"));
        layout.hits.push((SkillHit::Devotion, r));
        let tb = TextBox {
            style: None,
            rect: URect::new(x, y, r.w / ui.s, r.h / ui.s),
            align_x: gd_formats::ui::TextAlign::Center,
            align_y: gd_formats::ui::TextAlign::Center,
            tag: None,
            glow: None,
        };
        let title = ui.fonts.tag("tagSkillDevotion").to_string();
        ui.text_with_style(
            &tb,
            "records/ui/styles/text/style_special_classtabs.dbr",
            o,
            &title,
        );
    }
    let base = ui.record(BASE_PANE);
    let pane_o = o + Vec2::new(
        ui.sc(base.as_ref().and_then(|b| b.f32("panePosX")).unwrap_or(9.0)),
        ui.sc(base
            .as_ref()
            .and_then(|b| b.f32("panePosY"))
            .unwrap_or(44.0)),
    );
    let level = values.level as i32;
    let in_mastery = masteries.get(tab - 1).is_some();
    match masteries.get(tab - 1) {
        Some((n, tree)) => mastery_pane(
            ui,
            values,
            &field(&format!("skillCtrlPane{n}")),
            tree,
            pane_o,
            level,
            &mut layout,
        ),
        None => select_pane(
            ui,
            values,
            &field("masterySelectWindow"),
            pane_o,
            level,
            &mut layout,
        ),
    }
    // Undo button (`undoButton`, `ButtonStaticText`; the disabled bitmap while
    // nothing was spent since the window opened, `UISkillPane::Update`:
    // disabled flag = no pending change; click = the `UISkillPane` virtual
    // `UISkillPane::Undo`, see `gd_game::points`) and the points line (`pointsRemaining`; localized
    // `SkillPointsAvailable{,Singular}` / `NoSkillPointsAvailable` in `^r`,
    // GrimDawn.exe `UISkillPane::Update`). Mastery-pane only: the selection
    // table has no undo/points fields, so neither is drawn on a free slot.
    if in_mastery {
        if let Some(ur) = base
            .as_ref()
            .and_then(|b| b.str("undoButton").map(str::to_string))
            .and_then(|p| ui.record(&p))
            && let Some(Widget::Button {
                up, disabled, x, y, ..
            }) = Some(Widget::from_record(&ur))
            && let Some(b) = if values.skill_undo {
                up.or(disabled)
            } else {
                disabled.or(up)
            }
            && let Some((_, r)) = ui.bitmap(&b, x, y, pane_o)
        {
            if values.skill_undo {
                ui.record_button_sound(r, &ur);
                layout.hits.push((SkillHit::Undo, r));
            }
            let tb = TextBox {
                style: None,
                rect: URect::new(x, y, r.w / ui.s, r.h / ui.s),
                align_x: gd_formats::ui::TextAlign::Center,
                align_y: gd_formats::ui::TextAlign::Center,
                tag: None,
                glow: None,
            };
            let title = ur
                .str("textTag")
                .map(|t| ui.fonts.tag(t).to_string())
                .unwrap_or_default();
            if let Some(st) = ur.str("style").map(str::to_string) {
                ui.text_with_style(&tb, &st, pane_o, &title);
            }
        }
        if let Some(r) = base
            .as_ref()
            .and_then(|b| b.str("pointsRemaining").map(str::to_string))
            .and_then(|p| ui.record(&p))
        {
            let tb = TextBox::from_record(&r);
            let n = values.skill_points;
            let text = if n == 0 {
                format!("^r{}", ui.fonts.tag("NoSkillPointsAvailable"))
            } else {
                let tag = if n == 1 {
                    "SkillPointsAvailableSingular"
                } else {
                    "SkillPointsAvailable"
                };
                ui.fonts
                    .tag(tag)
                    .replace("{%d0}", &n.to_string())
                    .replace("%d0", &n.to_string())
            };
            ui.text(&tb, pane_o, &text);
        }
    }
    if in_mastery && values.reclaim.is_some() {
        reclaim_texts(ui, values, base.as_ref(), pane_o);
    }
    ui.commands.insert_resource(layout);
}

/// Reclamation mode's money and price (`UISkillPane::Draw` draws `playerGoldText`, `playerGoldNumberBitmap`,
/// `playerGoldNumber`, `costPerPoint`, `costPerPointNumberBitmap`, `costPerPointNumber` instead of the undo-mastery
/// button; `UISkillPane::Update` sets the numbers: the money (`^r` while 0) and `GetCurrentSkillReclamationCost`, both
/// `UI::FormatThousands` text). PORT: a `textstaticstring` label's `bitmapName` is drawn centred on its text box (its
/// `bitmapX/YPadding` use is not traced).
fn reclaim_texts(
    ui: &mut Ui,
    values: &HudValues,
    base: Option<&gd_formats::arz::Record>,
    po: Vec2,
) {
    let Some(r) = &values.reclaim else { return };
    let field = |k: &str| base.and_then(|b| b.str(k).map(str::to_string));
    let money = super::character::money_text(values.gold);
    let gold = if values.gold == 0 {
        format!("^r{money}")
    } else {
        money
    };
    let cost = super::character::money_text(gd_sim::reclamation::current_cost(&ui.data.db, r.used));
    for (label, bitmap, number, text) in [
        (
            "playerGoldText",
            "playerGoldNumberBitmap",
            "playerGoldNumber",
            gold,
        ),
        (
            "costPerPoint",
            "costPerPointNumberBitmap",
            "costPerPointNumber",
            cost,
        ),
    ] {
        if let Some(lr) = field(label).and_then(|p| ui.record(&p)) {
            let tb = TextBox::from_record(&lr);
            if let Some(b) = lr.str("bitmapName").map(str::to_string)
                && let Some((h, size)) = ui.image(&b)
            {
                let (w, hgt) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
                let c = Vec2::new(
                    po.x + ui.sc(tb.rect.x + tb.rect.w * 0.5),
                    po.y + ui.sc(tb.rect.y + tb.rect.h * 0.5),
                );
                ui.quad(
                    h,
                    URect::new(c.x - w * 0.5, c.y - hgt * 0.5, w, hgt),
                    None,
                    Color::WHITE,
                );
            }
            let t = lr
                .str("textTag")
                .map(|t| ui.fonts.tag(t).to_string())
                .unwrap_or_default();
            ui.text(&tb, po, &t);
        }
        if let Some(Widget::Bitmap { bitmap, x, y }) = field(bitmap).and_then(|p| ui.widget(&p)) {
            ui.bitmap(&bitmap, x, y, po);
        }
        if let Some(nr) = field(number).and_then(|p| ui.record(&p)) {
            ui.text(&TextBox::from_record(&nr), po, &text);
        }
    }
}

/// A skill button's reclaim flags in the mode (`gd_game::reclamation::flags_for`'s inputs from the view).
fn reclaim_flags(
    ui: &Ui,
    values: &HudValues,
    trees: &gd_sim::skills::SkillTrees,
    tree: &gd_sim::skills::ClassTree,
    skill: &str,
    full: bool,
) -> Option<gd_sim::reclamation::Flags> {
    let r = values.reclaim.as_ref()?;
    Some(gd_sim::reclamation::flags(
        &ui.data.db,
        &values.skills,
        trees,
        tree,
        skill,
        values.gold,
        r.used,
        full,
        r.is_bound(skill),
    ))
}

/// The mastery bar's (and tier beams') fill, `UISkillPane::GetMasteryBarFill`: each
/// passed tier counts 1, the tier the level lies in (`L[i-1] < level ≤ L[i]`,
/// `L[-1]` = 0) counts `(level − L[i-1]) / (L[i] − L[i-1])`, over the tier count.
pub fn tier_fraction(level: u32, tiers: &[u32]) -> f32 {
    if tiers.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    for (i, &t) in tiers.iter().enumerate() {
        let prev = if i == 0 { 0 } else { tiers[i - 1] };
        if level as f32 <= t as f32 {
            if t > prev {
                sum += level.saturating_sub(prev) as f32 / (t - prev) as f32;
            }
            break;
        }
        sum += 1.0;
    }
    sum / tiers.len() as f32
}

fn mastery_pane(
    ui: &mut Ui,
    values: &HudValues,
    table: &str,
    tree: &str,
    po: Vec2,
    level: i32,
    layout: &mut SkillLayout,
) {
    let Some(t) = ui.record(table) else { return };
    let f = |k: &str| t.str(k).map(str::to_string).unwrap_or_default();
    // The rules' per-tree data, derived once for every button of the pane.
    let data = ui.data;
    let trees = data.skill_trees();
    let class_tree = trees.tree(tree);
    let mastery = class_tree
        .and_then(|t| t.mastery())
        .unwrap_or_default()
        .to_string();
    let mlevel = values.skills.level(&mastery);
    let tiers = &trees.tiers;
    let frac = tier_fraction(mlevel, tiers);
    let full = masteries_allowed(values.level as i32) <= trees.masteries(&values.skills);
    let cannot_raise = |skill: &str| {
        class_tree.and_then(|t| {
            values
                .skills
                .cannot_raise(&data.db, &trees, t, skill, level)
        })
    };
    let bar = ui.record(&f("masteryBar"));
    let bar_image = bar
        .as_ref()
        .and_then(|b| b.str("bitmapFullName").map(str::to_string))
        .and_then(|p| ui.image(&p));
    // The tier beams (`UISkillPane::Draw`): before the background, a rect
    // in the pane's `peekThroughColor` (alpha 1) at the pane's x + 187 · s, as
    // tall as the background bitmap and as wide as the mastery bar × the tier
    // fraction (+ 20 · s while the level is exactly a tier level); it shows
    // through the background's transparent beam slits, so only unlocked tiers glow.
    let pt = Color::srgb(
        t.f32("peekThroughColorRed").unwrap_or(1.0),
        t.f32("peekThroughColorGreen").unwrap_or(1.0),
        t.f32("peekThroughColorBlue").unwrap_or(1.0),
    );
    if let (Some((_, bar_size)), Some(Widget::Bitmap { bitmap, .. })) =
        (bar_image.as_ref(), ui.widget(&f("skillPaneBaseBitmap")))
        && let Some((_, bg)) = ui.image(&bitmap)
    {
        let s = ui.s;
        let mut w = (bar_size.x as f32 * s).trunc() * frac;
        if tiers.contains(&mlevel) {
            w += s * 20.0;
        }
        if w > 0.0 {
            ui.solid(URect::new(po.x + s * 187.0, po.y, w, bg.y as f32 * s), pt);
        }
    }
    // The background is black with the beams in its alpha. Reclamation mode draws `skillPaneBaseReallocationBitmap`
    // and the base pane's `reallocationBitmap` (no bitmap in stock data) instead of the class image
    // (`UISkillPane::Draw`: `UISkillPane.baseReallocationBitmap` / `UISkillPane.reallocationBitmap` instead of
    // `UISkillPane.baseBitmap` / `UISkillPane.classImage`).
    let reclaim = values.reclaim.is_some();
    let back = if reclaim {
        "skillPaneBaseReallocationBitmap"
    } else {
        "skillPaneBaseBitmap"
    };
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&f(back))
        && let Some((h, size)) = ui.image(&bitmap)
    {
        let r = URect::new(
            po.x + ui.sc(x),
            po.y + ui.sc(y),
            ui.sc(size.x as f32),
            ui.sc(size.y as f32),
        );
        ui.quad(h, r, None, Color::WHITE);
    }
    let class_image = if reclaim {
        ui.record(BASE_PANE)
            .and_then(|b| b.str("reallocationBitmap").map(str::to_string))
    } else {
        Some(f("skillPaneMasteryBitmap"))
    };
    if let Some(Widget::Bitmap { bitmap, x, y }) = class_image.and_then(|p| ui.widget(&p))
        && !bitmap.is_empty()
    {
        ui.bitmap(&bitmap, x, y, po);
    }
    // Mastery bar (`bargraph.tpl`): the full bitmap cropped to the same tier
    // fraction (`UISkillPane::Update` sets the bar's value to `UISkillPane::GetMasteryBarFill`).
    if let (Some(bar), Some((h, size))) = (bar.as_ref(), bar_image)
        && frac > 0.0
    {
        let (x, y) = (
            bar.f32("bitmapPositionX").unwrap_or(0.0),
            bar.f32("bitmapPositionY").unwrap_or(0.0),
        );
        let dst = URect::new(
            po.x + ui.sc(x),
            po.y + ui.sc(y),
            ui.sc(size.x as f32 * frac),
            ui.sc(size.y as f32),
        );
        ui.quad(
            h,
            dst,
            Some(URect::new(0.0, 0.0, size.x as f32 * frac, size.y as f32)),
            Color::WHITE,
        );
    }
    let level_box = ui
        .record("records/ui/skills/classcommon/skillbuttons_skilllevelstyle.dbr")
        .map(|r| TextBox::from_record(&r));
    let base = ui.record(BASE_PANE);
    let bs = |k: &str| base.as_ref().and_then(|b| b.str(k).map(str::to_string));
    let buttons: Vec<(String, gd_formats::arz::Record)> = t
        .strs("tabSkillButtons")
        .into_iter()
        .filter_map(|b| ui.record(b).map(|r| (b.to_string(), r)))
        .collect();
    // Tier numbers on the bottom bar (`masteryMilestoneNumberN` text,
    // `skillMasteryTierLevel` values).
    for (i, lv) in tiers.iter().enumerate() {
        let Some(r) = bs(&format!("masteryMilestoneNumber{}", i + 1)).and_then(|p| ui.record(&p))
        else {
            continue;
        };
        let tb = TextBox::from_record(&r);
        ui.text(&tb, po, &lv.to_string());
    }
    let level_fmt = ui.fonts.tag("SkillLevelFormat").to_string();
    let level_fmt = if level_fmt.contains("d0") {
        level_fmt.replace("{%d0}", "%d0").replace("{%d1}", "%d1")
    } else {
        "%d0 / %d1".to_string()
    };
    // Connector textures (GrimDawn.exe `UISkillPane::Build` builds, `UISkillPane::Draw`
    // draws): the cells of a skill's `skillConnectionOn/Off` lists (equal
    // length, consumed in order) run from the skill's button across the
    // following buttons of `tabSkillButtons` while cells remain: each next
    // skill takes `max(tier gap, 0)` cells (+1 when the previous skill is a
    // branch skill, `isBranchSkill`), the budget shrinking by the signed tier
    // gap and 1 per branch skill. Cells start at (button x, button y - 20) in
    // pane space and advance by the texture width (not after the last cell
    // when the receiving skill is a branch skill). They belong to the
    // receiving skill and are drawn at pane origin + its button width (rect
    // width, `SkillButton.rectWidth`): "off" always, "on" when the chain's owner skill (the one
    // with the lists) is learned and the receiving skill's tier is open.
    {
        struct Entry {
            skill: String,
            x: f32,
            y: f32,
            w: f32,
            tier: i32,
            branch: bool,
            mastery: bool,
            on: Vec<String>,
            off: Vec<String>,
        }
        let entries: Vec<Entry> = buttons
            .iter()
            .filter_map(|(_, br)| {
                let skill = br.str("skillName")?.to_string();
                let rec = ui.data.db.get(&skill)?;
                let disp = gd_sim::skills::display_record(&ui.data.db, &skill);
                let tier = disp
                    .as_ref()
                    .and_then(|d| d.i32("skillTier"))
                    .or(rec.i32("skillTier"))
                    .unwrap_or(1);
                let w = br
                    .str("bitmapNameUp")
                    .and_then(|b| ui.image(b))
                    .map_or(0.0, |(_, s)| s.x as f32);
                // The connection lists live on the skill's resource record:
                // toggles and pets keep them on their buff / pet skill.
                let has = |r: &gd_formats::arz::Record| !r.strs("skillConnectionOn").is_empty();
                // Walks `buffSkillName` / `petSkillName` links to the record holding a field (not the profile lookup).
                let res = if has(rec) {
                    Some(rec)
                } else {
                    ["buffSkillName", "petSkillName"].iter().find_map(|k| {
                        rec.str(k)
                            .and_then(|p| ui.data.db.get(p))
                            .filter(|r| has(r))
                    })
                };
                let strs = |k: &str| {
                    res.as_ref()
                        .map(|r| {
                            r.strs(k)
                                .into_iter()
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                };
                Some(Entry {
                    on: strs("skillConnectionOn"),
                    off: strs("skillConnectionOff"),
                    x: br.f32("bitmapPositionX").unwrap_or(0.0),
                    y: br.f32("bitmapPositionY").unwrap_or(0.0),
                    w,
                    tier,
                    // `Skill::IsBranchSkill` (`Skill.isBranchSkill`, `isBranchSkill`);
                    // the transmuter classes override it to true.
                    branch: rec.i32("isBranchSkill").unwrap_or(0) != 0
                        || matches!(
                            rec.class.as_str(),
                            "Skill_Transmuter"
                                | "Skill_ProjectileTransmuter"
                                | "Skill_SpawnPetTransmuter"
                        ),
                    mastery: skill.eq_ignore_ascii_case(&mastery),
                    skill,
                })
            })
            .collect();
        // Per receiving entry: (off cells, on cells) as (texture, x, y).
        type Cell = (String, f32, f32);
        let mut cells: Vec<(Vec<Cell>, Vec<Cell>)> = vec![(Vec::new(), Vec::new()); entries.len()];
        // The chain owner of each receiving entry (`SkillPaneEntry.chainOwner`).
        let mut owner: Vec<Option<usize>> = vec![None; entries.len()];
        let width = |ui: &mut Ui, t: &str| ui.image(t).map_or(0.0, |(_, s)| s.x as f32);
        for (ei, e) in entries.iter().enumerate() {
            let mut on: std::collections::VecDeque<String> = e.on.iter().cloned().collect();
            let mut off: std::collections::VecDeque<String> = e.off.iter().cloned().collect();
            let mut budget = on.len() as i32;
            let mut chain = vec![ei];
            let mut cur = ei;
            while budget > 0 && chain.len() + ei <= entries.len() {
                let next = ei + chain.len();
                if next >= entries.len() {
                    break;
                }
                budget += entries[cur].tier - entries[next].tier;
                if entries[cur].branch {
                    budget -= 1;
                }
                chain.push(next);
                owner[next] = Some(ei);
                cur = next;
            }
            if chain.len() < 2 || off.is_empty() || on.len() != off.len() {
                continue;
            }
            let (mut px, py) = (e.x, e.y - 20.0);
            for pair in chain.windows(2) {
                let (xs, ys) = (&entries[pair[0]], &entries[pair[1]]);
                let n = (ys.tier - xs.tier).max(0) + xs.branch as i32;
                // Each link restarts both rows at the "off" row's end (`UISkillPane::Build`).
                let (mut ax, mut bx) = (px, px);
                for k in 0..n {
                    let last = k + 1 == n;
                    if let Some(t) = on.pop_front() {
                        cells[pair[1]].1.push((t.clone(), ax, py));
                        if !last || !ys.branch {
                            ax += width(ui, &t);
                        }
                    }
                    if let Some(t) = off.pop_front() {
                        cells[pair[1]].0.push((t.clone(), bx, py));
                        if !last || !ys.branch {
                            bx += width(ui, &t);
                        }
                    }
                }
                px = bx;
            }
        }
        for (i, (e, (offs, ons))) in entries.iter().zip(&cells).enumerate() {
            if ons.is_empty() || e.mastery {
                continue;
            }
            // Lit while the chain's owner skill is learned and this button's
            // tier is open (`UISkillPane::Draw`: the owner id at `SkillPaneEntry.chainOwner`).
            let open = cannot_raise(&e.skill) != Some("mastery level too low");
            let learned =
                owner[i].is_some_and(|o| values.skills.level(&entries[o].skill) > 0) && open;
            let draw = offs
                .iter()
                .chain(if learned { ons.iter() } else { [].iter() });
            for (tex, cx, cy) in draw {
                if let Some((h, size)) = ui.image(tex) {
                    let r = URect::new(
                        po.x + ui.sc(e.w + cx),
                        po.y + ui.sc(*cy),
                        ui.sc(size.x as f32),
                        ui.sc(size.y as f32),
                    );
                    ui.quad(h, r, None, Color::WHITE);
                }
            }
        }
    }
    for (_, br) in &buttons {
        let Some(skill) = br.str("skillName").map(str::to_string) else {
            continue;
        };
        let Some(sr) = gd_sim::skills::display_record(&ui.data.db, &skill) else {
            continue;
        };
        let (x, y) = (
            br.f32("bitmapPositionX").unwrap_or(0.0),
            br.f32("bitmapPositionY").unwrap_or(0.0),
        );
        let why = cannot_raise(&skill);
        let blocked = values.skill_points == 0 || why.is_some();
        let lvl = values.skills.level(&skill);
        let is_mastery = skill.eq_ignore_ascii_case(&mastery);
        // Level text: `SkillLevelFormat` of `GetCurrentLevel` (learned level +
        // equipment bonus, capped at the ultimate level) and `GetMaxLevel`
        // (GrimDawn.exe `UISkillPane::Update`). Blank while the mastery is
        // too low for the skill's tier.
        let shown = values.skills.effective_level(&ui.data.db, &skill);
        let own = ui.data.db.get(&skill);
        let max = own
            .as_ref()
            .and_then(|r| r.i32("skillMaxLevel"))
            .filter(|m| *m > 0)
            .or(sr.i32("skillMaxLevel"))
            .unwrap_or(1)
            .max(0) as u32;
        // Flags of `UISkillPane::GetRequirementFlags`: f1 mastery level too low, f2 parent not
        // learned, f3 maxed (`UISkillPane::IsMaxed`), f5 dependency unmet / f6 exclusive conflict
        // (`SkillBook::dependency_or_exclusive_blocked`, joins f2 in the red frame), f11 unlearned.
        let f1 = why == Some("mastery level too low");
        let f2 = why == Some("needs the parent skill")
            || values
                .skills
                .dependency_or_exclusive_blocked(&ui.data.db, &trees, &skill);
        let tip = gd_sim::skill_tooltip::SkillTipCtx {
            db: &data.db,
            tags: &ui.fonts.tags,
            book: &values.skills,
            player: None,
            trees: &trees,
            pet_bonuses: &[],
        };
        let f3 = gd_sim::skill_tooltip::is_maxed(&tip, &skill);
        // The "+ n" the tooltip shows: the level number takes `augmentFont`.
        let augmented = gd_sim::skill_tooltip::shown_bonus(&tip, &skill) > 0;
        let f11 = lvl == 0;
        // Reclamation mode (`gd_sim::reclamation::Flags`): the disabled icon when it takes no click, grey for
        // f1 / f2 / f4 / f5, a red / green frame.
        let rf = class_tree.and_then(|t| reclaim_flags(ui, values, &trees, t, &skill, full));
        let tint = if let Some(rf) = rf {
            if rf.grayed() {
                Color::srgb(0.35, 0.35, 0.35)
            } else {
                Color::WHITE
            }
        } else if lvl == 0 && blocked {
            Color::srgb(0.35, 0.35, 0.35)
        } else {
            Color::WHITE
        };
        let mut hit: Option<URect> = None;
        if let Some(border) = br.str("bitmapNameUp").map(str::to_string)
            && let Some((h, size)) = ui.image(&border)
        {
            let r = URect::new(
                po.x + ui.sc(x),
                po.y + ui.sc(y),
                ui.sc(size.x as f32),
                ui.sc(size.y as f32),
            );
            ui.quad(h, r, None, tint);
            hit = Some(r);
        }
        let (ox, oy) = (
            br.f32("skillOffsetX").unwrap_or(0.0),
            br.f32("skillOffsetY").unwrap_or(0.0),
        );
        if let Some(icon) = sr
            .str(if rf.map_or(lvl > 0, |f| !f.disabled()) {
                "skillUpBitmapName"
            } else {
                "skillDownBitmapName"
            })
            .or(sr.str("skillUpBitmapName"))
            .map(str::to_string)
            && let Some((h, size)) = ui.image(&icon)
        {
            let r = URect::new(
                po.x + ui.sc(x + ox),
                po.y + ui.sc(y + oy),
                ui.sc(size.x as f32),
                ui.sc(size.y as f32),
            );
            ui.quad(h, r, None, tint);
            hit = hit.or(Some(r));
        }
        // Frame colour (`UISkillPane::Draw`, normal mode): maxed blue; no points
        // left: learned gold; points left: red when the parent is missing,
        // green when the tier is open; the round set for `isCircular`.
        if !is_mastery {
            let frame = if let Some(rf) = rf {
                Some(if rf.red() { "red" } else { "green" })
            } else if f3 {
                Some("blue")
            } else if values.skill_points == 0 {
                (!f11).then_some("gold")
            } else if f2 && !f1 {
                Some("red")
            } else if !f1 {
                Some("green")
            } else {
                None
            };
            if let Some(c) = frame {
                let key = format!(
                    "{c}{}Bitmap",
                    if br.f32("isCircular").unwrap_or(0.0) > 0.0 {
                        "Round"
                    } else {
                        ""
                    }
                );
                if let Some(Widget::Bitmap {
                    bitmap,
                    x: fx,
                    y: fy,
                }) = bs(&key).and_then(|p| ui.widget(&p))
                {
                    ui.bitmap(&bitmap, x + fx, y + fy, po);
                }
            }
        }
        let Some(r) = hit else { continue };
        // PORT: the press sound only while a point can be spent (the button's enabled state was not traced).
        if values.skill_points > 0 {
            ui.record_button_sound(r, br);
        }
        layout.hits.push((
            SkillHit::Raise {
                tree: tree.to_string(),
                skill: skill.clone(),
            },
            r,
        ));
        if let Some(tb) = &level_box
            && !is_mastery
            && !f1
        {
            // `unavailableFont`/`emptyAugmentFont` while unlearned, else
            // `augmentFont`, `completeFont`, or the default style.
            let font = if f11 {
                bs(if augmented {
                    "emptyAugmentFont"
                } else {
                    "unavailableFont"
                })
            } else if augmented {
                bs("augmentFont")
            } else if f3 {
                bs("completeFont")
            } else {
                None
            };
            let text = level_fmt
                .replace("%d0", &shown.to_string())
                .replace("%d1", &max.to_string());
            let at = Vec2::new(po.x + ui.sc(x), po.y + ui.sc(y));
            match font {
                Some(f) => ui.text_with_style(tb, &f, at, &text),
                None => ui.text(tb, at, &text),
            };
        }
    }
    // No mastery level number: the pane loads `masterySkillLevel`
    // (GrimDawn.exe `UISkillPane::Load` → `UISkillPane.masterySkillLevel`) but nothing reads it again
    // (only the constructor and destructor touch the field).
}

fn select_pane(
    ui: &mut Ui,
    values: &HudValues,
    table: &str,
    po: Vec2,
    level: i32,
    layout: &mut SkillLayout,
) {
    let Some(t) = ui.record(table) else { return };
    if let Some(Widget::Bitmap { bitmap, x, y }) =
        ui.widget(t.str("masteryBaseBitmap").unwrap_or(""))
    {
        ui.bitmap(&bitmap, x, y, po);
    }
    let trees = class_trees(&ui.data.db);
    let chosen: Vec<String> = ui
        .data
        .skill_trees()
        .learned(&values.skills)
        .map(|t| t.path.clone())
        .collect();
    let free = masteries_allowed(level) > chosen.len() && values.skill_points > 0;
    let texts: Vec<String> = t
        .strs("masteryMasteryText")
        .into_iter()
        .map(str::to_string)
        .collect();
    for (i, b) in t
        .strs("masteryMasteryButtons")
        .into_iter()
        .map(str::to_string)
        .enumerate()
    {
        let Some((_, tree)) = trees.iter().find(|(n, _)| *n as usize == i + 1) else {
            continue;
        };
        let Some(Widget::Button {
            up, disabled, x, y, ..
        }) = ui.widget(&b)
        else {
            continue;
        };
        let usable = free && !chosen.contains(tree);
        let bmp = if usable { up } else { disabled.or(up) };
        let Some((_, r)) = bmp.and_then(|b| ui.bitmap(&b, x, y, po)) else {
            continue;
        };
        if usable && let Some(mastery) = tree_skills(&ui.data.db, tree).first().cloned() {
            button_sound(ui, r, &b);
            layout.hits.push((
                SkillHit::Raise {
                    tree: tree.clone(),
                    skill: mastery,
                },
                r,
            ));
        }
        if let Some(tr) = texts.get(i).and_then(|p| ui.record(p)) {
            let tag = tr
                .str("textTag")
                .map(|t| ui.fonts.tag(t).to_string())
                .unwrap_or_default();
            // Text boxes are pane-relative, like the buttons.
            let _ = r;
            ui.text(&TextBox::from_record(&tr), po, &tag);
        }
    }
}

/// The windows drawn above this one that its policy row leaves open: the local map and the quest journal.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Above<'w> {
    map: Res<'w, crate::state::map::MapWindow>,
    rift: Res<'w, gd_game::riftgates::RiftgateMap>,
    journal: Res<'w, super::quests::QuestJournal>,
    journal_layout: Res<'w, super::quests::JournalLayout>,
}

impl Above<'_> {
    /// A higher window's rect contains `p` (physical px).
    pub fn covers(&self, p: Vec2) -> bool {
        self.map.covers(&self.rift, p) || (self.journal.open && self.journal_layout.covers(p))
    }
}

/// Clicks on the skills window (runs after the inventory's, before movement).
#[allow(clippy::too_many_arguments)]
pub fn clicks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut panels: ResMut<super::UiPanels>,
    (layout, above): (Res<SkillLayout>, Above),
    mut capture: ResMut<UiCapture>,
    time: Res<Time>,
    mut script: Local<Option<Vec<gd_game_core::hooks::Click>>>,
    (mut raise, mut undo, mut commit, mut refund, reclaim): (
        MessageWriter<gd_game::points::RaiseSkill>,
        MessageWriter<gd_game::points::UndoSkillPoints>,
        MessageWriter<gd_game::points::CommitSkillUndo>,
        MessageWriter<gd_game::reclamation::ReclaimRequest>,
        // Option: created with the player (`gd_game::reclamation::init_reclamation`).
        Option<Res<gd_game::reclamation::SkillReclamation>>,
    ),
    mut was_open: Local<bool>,
) {
    // The window hid (`UISkillPane::OnHide`): the spent points are final.
    if *was_open && !panels.skills {
        commit.write(gd_game::points::CommitSkillUndo);
    }
    *was_open = panels.skills;
    if !panels.skills {
        return;
    }
    let Ok(win) = window.single() else { return };
    // `GD_SKILL_CLICKS` (hooks.rs): physical px.
    let script = script.get_or_insert_with(|| hooks.ui.skill_clicks.clone());
    let scripted = gd_game_core::hooks::take_due(script, time.elapsed_secs()).map(|c| c.at);
    let (p, clicked) = match scripted {
        Some(p) => (p, true),
        None => match super::cursor_px(&hooks, win) {
            Some(c) => (c, super::widgets::button_pressed(&buttons)),
            None => return,
        },
    };
    let Some(wr) = layout.window else { return };
    if !contains(&wr, p) || above.covers(p) {
        return;
    }
    capture.0 = true;
    if !clicked {
        return;
    }
    let Some((hit, _)) = layout.hits.iter().find(|(_, r)| contains(r, p)) else {
        return;
    };
    match hit {
        SkillHit::Close => panels.skills = false,
        SkillHit::Tab(t) => panels.skill_tab = *t,
        // In the mode the tab moves it to the devotion window (`gd_game::reclamation`, "Switching"); the answer
        // opens it.
        SkillHit::Devotion if reclaim.as_ref().is_some_and(|r| r.skills_mode()) => {
            refund.write(gd_game::reclamation::ReclaimRequest::Switch { devotion: true });
        }
        SkillHit::Devotion => {
            panels.skills = false;
            panels.devotion = true;
        }
        SkillHit::Undo => {
            undo.write(gd_game::points::UndoSkillPoints);
        }
        // In reclamation mode a click takes a point back (`gd_game::reclamation`).
        SkillHit::Raise { tree, skill } if reclaim.as_ref().is_some_and(|r| r.skills_mode()) => {
            refund.write(gd_game::reclamation::ReclaimRequest::Refund {
                tree: tree.clone(),
                skill: skill.clone(),
            });
        }
        SkillHit::Raise { tree, skill } => {
            raise.write(gd_game::points::RaiseSkill {
                tree: tree.clone(),
                skill: skill.clone(),
            });
        }
    }
}

/// What the tooltip currently shows: (tree, skill, skills, points, level, screen, reclamation mode and money).
/// Compared with the live values by reference; copied only when the tooltip is redrawn.
type TipKey = (
    String,
    String,
    gd_sim::skills::SkillBook,
    u32,
    u32,
    UVec2,
    Option<(super::reclamation::ReclaimView, u64)>,
);

/// Hover tooltip of a skill button (`GenerateUISkillText`, see `gd_sim::skill_tooltip`), shown through the shared
/// [`super::tip::TipCanvas`] so hovering does not rebuild the window. `GD_SKILL_HOVER` (hooks.rs, physical px) fakes the cursor.
#[allow(clippy::too_many_arguments)]
pub fn tooltip(
    hooks: Res<gd_game_core::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    panels: Res<super::UiPanels>,
    (layout, above): (Res<SkillLayout>, Above),
    values: Res<HudValues>,
    mut canvas: super::tip::TipCanvas,
    mut fresh: gd_game_core::session::NewSession,
    mut shown: Local<Option<TipKey>>,
    player: Query<
        (
            &gd_game_core::combat::Combatant,
            &gd_game_core::player::Loadout,
        ),
        With<gd_game_core::player::Player>,
    >,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
) {
    if fresh.begun() {
        *shown = Default::default();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let cursor = hooks
        .ui
        .skill_hover
        .or_else(|| super::cursor_px(&hooks, win))
        .filter(|p| !above.covers(*p));

    let hover = match (&layout, cursor) {
        (l, Some(p)) if panels.skills => l.hits.iter().find_map(|(h, r)| match h {
            SkillHit::Raise { tree, skill } if contains(r, p) => Some((tree, skill, *r)),
            _ => None,
        }),
        _ => None,
    };
    let same = match (&*shown, &hover) {
        (None, None) => true,
        (Some(k), Some((t, s, _))) => {
            k.0 == **t
                && k.1 == **s
                && k.2 == values.skills
                && k.3 == values.skill_points
                && k.4 == values.level
                && k.5 == size
                && k.6.as_ref().map(|(r, g)| (r, *g))
                    == values.reclaim.as_ref().map(|r| (r, values.gold))
        }
        _ => false,
    };
    if same {
        return;
    }
    *shown = hover.as_ref().map(|(t, s, _)| {
        (
            (*t).clone(),
            (*s).clone(),
            values.skills.clone(),
            values.skill_points,
            values.level,
            size,
            values.reclaim.clone().map(|r| (r, values.gold)),
        )
    });
    canvas.clear(super::tip::TipOwner::Skills);
    let Some((tree, skill, anchor)) = hover else {
        return;
    };
    if size.x == 0 || size.y == 0 {
        return;
    }
    let s = canvas.assets.scale(size);
    let lines = {
        let (data, fonts) = (&canvas.data, &canvas.assets.fonts);
        let pet_bonuses = player
            .iter()
            .next()
            .map(|(_, l)| gd_game::pets::pet_bonuses(data, l))
            .unwrap_or_default();
        let trees = data.skill_trees();
        let ctx = gd_sim::skill_tooltip::SkillTipCtx {
            db: &data.db,
            trees: &trees,
            tags: &fonts.tags,
            book: &values.skills,
            player: rules
                .as_ref()
                .zip(player.iter().next())
                .map(|(r, (c, _))| (&*r.0, &*c.profile)),
            pet_bonuses: &pet_bonuses,
        };
        let chosen = trees.masteries(&values.skills);
        let full = masteries_allowed(values.level as i32) <= chosen;
        let mut reasons =
            gd_sim::skill_tooltip::reasons(&ctx, tree, skill, values.skill_points, full);
        if let Some(r) = &values.reclaim
            && let Some(t) = trees.tree(tree)
        {
            let f = gd_sim::reclamation::flags(
                &data.db,
                &values.skills,
                &trees,
                t,
                skill,
                values.gold,
                r.used,
                full,
                r.is_bound(skill),
            );
            reasons.reclaim = Some((f.tip(), gd_sim::reclamation::current_cost(&data.db, r.used)));
        }
        gd_sim::skill_tooltip::skill_lines(&ctx, skill, Some(&reasons))
    };
    if lines.is_empty() {
        return;
    }
    for l in &lines {
        debug!("skill tip: {}", l.text);
    }
    canvas.show(
        super::tip::TipOwner::Skills,
        &lines,
        super::tip::Blank::DefaultLine,
        |w, h, screen, gap| {
            // PORT: centred above the button when it
            // fits, else beside it (left, or right off the screen's left edge).
            let above = anchor.y - h - gap;
            if above >= 0.0 {
                return (
                    (anchor.x + anchor.w * 0.5 - w * 0.5).clamp(0.0, (screen.x - w).max(0.0)),
                    above,
                );
            }
            let mut x = anchor.x - w - gap;
            if x < 0.0 {
                x = anchor.x + anchor.w + gap;
            }
            (
                x.clamp(0.0, (screen.x - w).max(0.0)),
                (anchor.y + anchor.h - h + 16.0 * s).clamp(0.0, (screen.y - h).max(0.0)),
            )
        },
    );
}
