//! Character window (`records/ui/character/character_mastertable.dbr`,
//! loaders `Grim Dawn.exe` `CharacterWindow::LoadEquipment` + `CharacterWindow::Load`): equipment, inventory
//! and the character-stats tab pane. Opened with `I` or `C`.
//!
//! The window uses the generic window rect (`windowScreenAlignmentX Right`,
//! `Y Center`); all children are window-relative. The stats tab pane sits at
//! its record's `vecX/vecY` and its own children are pane-relative.
//!
//! PORT: the stats list's label / value columns are measured from the text, labels and values truncate
//! to their column and the row pitch follows the font line height (stock shares one fixed box).
use super::{HudValues, Ui};
use crate::state::inventory::{InvLayout, SheetList};
use bevy::prelude::*;
use gd_formats::inventory::EquipSlot;
use gd_formats::ui::{Rect as URect, Widget, WindowDef, round_px};
use gd_game_core::inventory::Inventory;

pub const MASTER: &str = "records/ui/character/character_mastertable.dbr";

/// The record a master-table field names (empty when missing).
pub fn field_record(data: &gd_game_core::data::GameData, k: &str) -> String {
    data.db
        .get(MASTER)
        .and_then(|m| m.str(k).map(str::to_owned))
        .unwrap_or_default()
}

/// Character-window box of each equipment slot.
const SLOT_FIELDS: [(&str, EquipSlot); 14] = [
    ("equipHead", EquipSlot::Head),
    ("equipNeck", EquipSlot::Amulet),
    ("equipArtifact", EquipSlot::Relic),
    ("equipChest", EquipSlot::Chest),
    ("equipLegs", EquipSlot::Legs),
    ("equipFeet", EquipSlot::Feet),
    ("equipHands", EquipSlot::Hands),
    ("equipWaist", EquipSlot::Waist),
    ("equipShoulders", EquipSlot::Shoulders),
    ("equipHandLeft", EquipSlot::OffHand),
    ("equipHandRight", EquipSlot::MainHand),
    ("equipFinger1", EquipSlot::Ring1),
    ("equipFinger2", EquipSlot::Ring2),
    ("equipMedal", EquipSlot::Medal),
];

/// Draws an item's bitmap centred in `r`.
pub(crate) fn item_icon(
    ui: &mut Ui,
    inv: &Inventory,
    item: &gd_formats::save::item::Item,
    r: URect,
) {
    let Some(info) = inv.info(item) else { return };
    let Some((h, size)) = ui.image(&info.resolved.bitmap) else {
        return;
    };
    let (sw, sh) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
    let d = URect::new(
        r.x + ((r.w - sw) * 0.5).trunc(),
        r.y + ((r.h - sh) * 0.5).trunc(),
        sw,
        sh,
    );
    ui.quad(h, d, None, Color::WHITE);
}

/// `gameiteminfo.dbr` field prefix of a display colour class
/// (`ResolvedItem::display_color_index`; `GameEngine::LoadFromDatabase`
/// reads `<prefix>ItemBackground` (default `ui/character/itembackground.tex`)
/// and `<prefix>ItemColor.r/g/b/a`).
pub fn rarity_key(index: u32) -> Option<&'static str> {
    Some(match index {
        0 => "common",
        1 => "magical",
        2 => "rare",
        3 => "epic",
        4 => "legendary",
        5 => "broken",
        6 => "potion",
        8 => "relic",
        9 => "quest",
        // `GameEngine::LoadFromDatabase` (map keys 10..14).
        10 | 12 => "artifact",
        11 => "artifactFormula",
        13 => "lore",
        14 => "enchantment",
        16 => "doubleRare",
        17 => "monsterDoubleRare",
        _ => return None,
    })
}

/// `gameiteminfo.dbr`'s colour field of a [`rarity_key`]: `<key>ItemColor`, but `artifactColor` /
/// `artifactFormulaColor` for the crafted-relic and blueprint classes (`GameEngine::LoadFromDatabase`
/// reads those names). The `.r/g/b/a` floats (the item background's tint) and the text colour code
/// (the name's, [`name_color`]) both live under it.
pub(crate) fn color_field(key: &str) -> String {
    if key.starts_with("artifact") {
        format!("{key}Color")
    } else {
        format!("{key}ItemColor")
    }
}

/// An item name's colour by its display class (`ResolvedItem::display_color_index`): the colour code
/// `Item::GetGameDescription` puts in front of the name (Game.dll, its call:
/// `GameEngine::GetItemColorText(GetDropClassification())`), i.e. the text of [`color_field`] through the text
/// colour table (`gd_formats::text::color_code`): a blueprint and a component `{^o}` orange, a crafted relic
/// `{^c}` cyan; white for a class without one. The ground labels and the crafting window's names use it.
pub fn name_color(db: &gd_formats::vfs::Database, index: u32) -> [f32; 3] {
    rarity_key(index)
        .zip(db.get("records/game/gameiteminfo.dbr"))
        .and_then(|(key, rec)| {
            let code = rec
                .str(&color_field(key))?
                .trim_matches(|c| c == '{' || c == '}');
            let c = gd_formats::text::color_code(code.strip_prefix('^')?.chars().next()?)?;
            Some([c[0], c[1], c[2]])
        })
        .unwrap_or([1.0; 3])
}

/// An item in a grid cell / equipment slot `r`: the rarity background
/// (texture tinted by the class colour), the icon, the unusable mark
/// (`ui/character/itemunusablemark.tex` at the bottom left, 4 px in; exe
/// `ItemIcon::Render`) and the stack size.
pub(crate) fn item_cell(
    ui: &mut Ui,
    inv: &Inventory,
    item: &gd_formats::save::item::Item,
    r: URect,
) {
    if let Some(info) = inv.info(item)
        && let Some(key) = rarity_key(info.resolved.display_color_index())
        && let Some(rec) = ui.data.db.get("records/game/gameiteminfo.dbr")
    {
        let c = |k: &str| rec.f32(&format!("{}.{k}", color_field(key))).unwrap_or(0.0);
        let color = Color::srgba(c("r"), c("g"), c("b"), c("a"));
        let tex = rec
            .str(&format!("{key}ItemBackground"))
            .unwrap_or("ui/character/itembackground.tex")
            .to_string();
        if c("a") > 0.0
            && let Some((h, _)) = ui.image(&tex)
        {
            ui.quad_stretch(h, r, color);
        }
    }
    item_icon(ui, inv, item, r);
    if !inv.usable(item, &ui.req)
        && let Some((h, size)) = ui.image("ui/character/itemunusablemark.tex")
    {
        let (w, hh) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
        ui.quad(
            h,
            URect::new(r.x + 4.0, r.y + r.h - hh - 4.0, w, hh),
            None,
            Color::WHITE,
        );
    }
    if item.stack_count > 1 {
        let tb = gd_formats::ui::TextBox {
            style: None,
            rect: gd_formats::ui::Rect::new(
                (r.x + 2.0) / ui.s,
                (r.y + 2.0) / ui.s,
                (r.w - 4.0) / ui.s,
                (r.h - 2.0) / ui.s,
            ),
            align_x: gd_formats::ui::TextAlign::End,
            align_y: gd_formats::ui::TextAlign::End,
            tag: None,
            glow: None,
        };
        ui.text_with_style(
            &tb,
            "records/ui/styles/text/style_textwhite_sizes_bold.dbr",
            Vec2::ZERO,
            &item.stack_count.to_string(),
        );
    }
}

/// An item in an inventory / stash / vendor grid: [`item_cell`] over the
/// grid's `failsRequirementsColor` plate when the player cannot use it
/// (the grid draw GrimDawn.exe `BagWidget::Render`: the plate is drawn
/// first for items flagged as failing requirements; the colour is the grid
/// record's, 0.2 0.02 0.02 0.5 in `inventory_grid0.dbr`).
pub(crate) fn grid_item(
    ui: &mut Ui,
    inv: &Inventory,
    item: &gd_formats::save::item::Item,
    r: URect,
) {
    if !inv.usable(item, &ui.req)
        && let Some(g) = ui
            .data
            .db
            .get("records/ui/character/characterinventory/inventory_grid0.dbr")
    {
        let c = |k: &str, d: f32| g.f32(&format!("failsRequirementsColor{k}")).unwrap_or(d);
        ui.solid(
            r,
            Color::srgba(
                c("Red", 0.2),
                c("Green", 0.02),
                c("Blue", 0.02),
                c("Alpha", 0.5),
            ),
        );
    }
    item_cell(ui, inv, item, r);
}

pub fn build(
    ui: &mut Ui,
    values: &HudValues,
    vitals: &super::HudVitals,
    panels: &super::UiPanels,
    inv: Option<&Inventory>,
) {
    let tab = panels.tab.max(1);
    let Some(m) = ui.record(MASTER) else { return };
    let field = |k: &str| m.str(k).map(str::to_string).unwrap_or_default();
    let mut w = WindowDef::from_record(&m).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    if let Some((_, x)) = ui.docked {
        w.x = x;
    }
    let o = Vec2::new(w.x, w.y);
    ui.sound_blocker(w);
    let mut layout = InvLayout {
        window: Some(w),
        ..Default::default()
    };

    ui.bitmap_field(&field("characterDisplayBitmap"), o);
    if let Some(Widget::SceneView { rect, .. }) = ui.widget(&field("characterView")) {
        let r = URect::new(
            o.x + ui.sc(rect.x),
            o.y + ui.sc(rect.y),
            ui.sc(rect.w),
            ui.sc(rect.h),
        );
        crate::doll::panel(
            ui,
            crate::doll::DollView::Character,
            &field("characterView"),
            r,
        );
        layout.doll = Some(r);
    }
    for (k, slot) in SLOT_FIELDS {
        if let Some(Widget::ItemBox { rect, silhouette }) = ui.widget(&field(k)) {
            let r = URect::new(
                o.x + ui.sc(rect.x),
                o.y + ui.sc(rect.y),
                ui.sc(rect.w),
                ui.sc(rect.h),
            );
            layout.slots.push((slot, r));
            let worn = inv.and_then(|i| {
                i.equipped[gd_game_core::inventory::SLOTS
                    .iter()
                    .position(|s| *s == slot)
                    .unwrap()]
                .as_ref()
            });
            match (worn, inv) {
                (Some(item), Some(inv)) => item_cell(ui, inv, item, r),
                _ => item_box(ui, rect, silhouette.as_deref(), o),
            }
        }
    }
    let rotate = ["rotateLeftButton", "rotateRightButton"].map(|k| {
        let w = ui.widget(&field(k));
        crate::doll::rotate_button(ui, w, o)
    });
    layout.close_button = super::window::close_button(ui, &field("characterCloseButton"), o);
    crate::doll::rotate_buttons(ui, crate::doll::DollView::Character, rotate);
    // The weapon-swap button (the loader reads only `equipSwap1LeftButton`): the down bitmap while
    // set II is active (`CharacterWindow::Update` / `ButtonStatic::Render`).
    if let Some(Widget::Button {
        up: Some(up),
        down,
        x,
        y,
        ..
    }) = ui.widget(&field("equipSwap1LeftButton"))
    {
        let alt = inv.is_some_and(|i| i.alternate);
        let bitmap = down.filter(|_| alt).unwrap_or(up);
        if let Some((_, r)) = ui.bitmap(&bitmap, x, y, o) {
            ui.record_path_button_sound(r, &field("equipSwap1LeftButton"));
            layout.swap_button = Some(r);
        }
    }

    // Inventory: the main grid, and the selected extra bag with its tab's panel image only once the character
    // has one (`CharacterWindow::RenderInventory`: the panel of the selected sack, none for sack 0; the tab
    // buttons and the second auto-sort button only while `PlayerInventoryCtrl::GetNumberOfSacks` > 1).
    let bag_count = inv.map_or(1, |i| i.bags.len());
    let shown = inv.map(|i| i.bag_tab).filter(|&t| t >= 1 && t < bag_count);
    if let Some(t) = shown {
        ui.bitmap_field(&field(&format!("inventoryTab{t}Bitmap")), o);
    }
    for i in 1..bag_count.min(6) {
        if let Some(Widget::Button { x, y, .. }) =
            ui.widget(&field(&format!("inventoryTab{i}Button")))
        {
            let t = if Some(i) == shown {
                field("inventoryTabActive")
            } else {
                field("inventoryTabInactive")
            };
            if let Some((_, r)) = ui.bitmap(&t, x, y, o) {
                ui.record_path_button_sound(r, &field(&format!("inventoryTab{i}Button")));
                layout.bag_tabs.push((i, r));
            }
        }
    }
    for (b, k) in
        std::iter::once((0usize, "inventoryGrid0")).chain(shown.map(|t| (t, "inventoryGrid1")))
    {
        let Some(Widget::InventoryGrid { rect }) = ui.widget(&field(k)) else {
            continue;
        };
        let r = URect::new(
            o.x + ui.sc(rect.x),
            o.y + ui.sc(rect.y),
            ui.sc(rect.w),
            ui.sc(rect.h),
        );
        let Some(inv) = inv else {
            continue;
        };
        let Some(bag) = inv.bags.get(b) else {
            continue;
        };
        let cell = r.w / bag.width as f32;
        layout.grids.push((b, r, cell));
        for p in &bag.items {
            let cr = URect::new(
                r.x + p.x as f32 * cell,
                r.y + p.y as f32 * cell,
                p.size.w as f32 * cell,
                p.size.h as f32 * cell,
            );
            grid_item(ui, inv, &p.item, cr);
            if inv.fresh.contains(&(p.item.base_name.clone(), p.item.seed)) {
                let e = ui.solid(cr, Color::srgba(1.0, 1.0, 1.0, 0.0));
                ui.commands
                    .entity(e)
                    .try_insert(super::newitems::NewItemGlow {
                        key: (p.item.base_name.clone(), p.item.seed),
                        rect: cr,
                    });
            }
        }
    }
    if let Some(Widget::Text(tb)) = ui.widget(&field("characterGoldNumber")) {
        let text = money_text(values.gold);
        let shown = fit_sheet(ui, &tb, &text);
        if let Some(e) = ui.text(&tb, o, &shown) {
            ui.commands.entity(e).insert(SheetVital {
                source: VitalSource::Gold,
                tb,
                po: o,
                shown: text,
                s: ui.s,
                inv: ui.inv,
            });
        }
    }

    // Under the grids: auto-sort of the main bag (left) and of the selected
    // extra bag (right, only with one), and the move-reagents button between them.
    for (k, which) in [
        ("invSortButton1", Some(0usize)),
        ("reagentDepositButton", None),
        ("invSortButton2", shown),
    ] {
        if k == "invSortButton2" && which.is_none() {
            continue;
        }
        if let Some(Widget::Button {
            up: Some(up), x, y, ..
        }) = ui.widget(&field(k))
            && let Some((_, r)) = ui.bitmap(&up, x, y, o)
        {
            match which {
                Some(b) => {
                    ui.record_path_button_sound(r, &field(k));
                    layout.sort_buttons.push((b, r));
                }
                None => layout.reagent_button = Some(r),
            }
        }
    }

    // Page buttons: the hidden `characterTabNButton`s take the clicks; the window draws only the
    // open page's `tabNBitmap` (`CharacterWindow::Render`: `pages[selected]`).
    for n in 1..=3u8 {
        if let Some(Widget::Button { x, y, w, h, .. }) =
            ui.widget(&field(&format!("characterTab{n}Button")))
        {
            let r = URect::new(o.x + ui.sc(x), o.y + ui.sc(y), ui.sc(w), ui.sc(h));
            ui.record_path_button_sound(r, &field(&format!("characterTab{n}Button")));
            layout.sheet_tabs.push((n, r));
        }
        if n != tab {
            continue;
        }
        let Some(t) = ui.record(&field(&format!("characterTab{n}"))) else {
            continue;
        };
        let Some(Widget::Bitmap { bitmap, x, y }) =
            t.str(&format!("tab{n}Bitmap")).and_then(|p| ui.widget(p))
        else {
            continue;
        };
        let po = o + Vec2::new(
            ui.sc(t.f32("vecX").unwrap_or(0.0)),
            ui.sc(t.f32("vecY").unwrap_or(0.0)),
        );
        ui.bitmap(&bitmap, x, y, po);
    }

    let tab_rec = field(&format!("characterTab{tab}"));
    if let Some(t) = ui.record(&tab_rec) {
        let po = o + Vec2::new(
            ui.sc(t.f32("vecX").unwrap_or(0.0)),
            ui.sc(t.f32("vecY").unwrap_or(0.0)),
        );
        // The rows live in the page's scroll window (`tabNScrollWindow` positionX/Y).
        let sw = t
            .str(&format!("tab{tab}ScrollWindow"))
            .and_then(|p| ui.record(p));
        let content = po
            + sw.map_or(Vec2::ZERO, |w| {
                Vec2::new(
                    ui.sc(w.f32("positionX").unwrap_or(0.0)),
                    ui.sc(w.f32("positionY").unwrap_or(0.0)),
                )
            });
        if tab >= 2 {
            stat_list(ui, &t, tab, content, values, panels, &mut layout);
        } else {
            stats_pane(ui, &t, content, values, vitals, &mut layout);
        }
    }
    ui.commands.insert_resource(layout);
}

/// A page II / III list entry: section header, spacer (`true` = the big one
/// after a header), row, or a row of the skill block (page II top).
#[derive(Clone, Copy)]
enum It {
    H(&'static str),
    G(bool),
    R(&'static str),
    S,
    /// The greatest monster killed (a name line under its label).
    M,
}
use It::{G, H, M, R, S};

/// Row order of `tab3ListBox`, from the list builder
/// GrimDawn.exe `CharacterSheetPage3::BuildList` (every row is added unconditionally; the
/// spacers are `10 * scale`, `13 * scale` after a header).
const TAB3: &[It] = &[
    H("HeaderDefenseText"),
    G(true),
    R("ChanceToBlockText"),
    R("DamageBlockedText"),
    R("BlockRecoveryText"),
    R("DodgeChanceText"),
    R("ProjDeflectChanceText"),
    G(false),
    R("StunResistText"),
    R("DisruptionResistText"),
    R("LifeLeechResistText"),
    R("EnergyLeechResistText"),
    R("TrapResistText"),
    R("PetrifyResistText"),
    R("FreezeResistText"),
    R("SlowResistText"),
    R("ReflectResistText"),
    G(false),
    H("HeaderRetaliationText"),
    G(true),
    R("PhysicalRetText"),
    R("PhysicalPercentRetText"),
    G(false),
    R("PierceRetText"),
    R("PiercePercentRetText"),
    G(false),
    R("BleedRetText"),
    R("BleedPercentRetText"),
    G(false),
    R("InternalTraumaRetText"),
    R("InternalTraumaPercentRetText"),
    G(false),
    R("FireRetText"),
    R("FirePercentRetText"),
    G(false),
    R("ColdRetText"),
    R("ColdPercentRetText"),
    G(false),
    R("LightningRetText"),
    R("LightningPercentRetText"),
    G(false),
    R("AcidRetText"),
    R("AcidPercentRetText"),
    G(false),
    R("VitalityRetText"),
    R("VitalityPercentRetText"),
    G(false),
    R("AetherRetText"),
    R("AetherPercentRetText"),
    G(false),
    R("ChaosRetText"),
    R("ChaosPercentRetText"),
    G(false),
    R("BurnRetText"),
    R("BurnPercentRetText"),
    G(false),
    R("ColdburnRetText"),
    R("ColdburnPercentRetText"),
    G(false),
    R("ElectrocuteRetText"),
    R("ElectrocutePercentRetText"),
    G(false),
    R("PoisonRetText"),
    R("PoisonPercentRetText"),
    G(false),
    R("VitalityDecayRetText"),
    R("VitalityDecayPercentRetText"),
    G(false),
    H("HeaderStatsText"),
    G(true),
    R("ItemLevelText"),
    R("TotalPlaytimeText"),
    R("MonstersKilledText"),
    R("ChampionsKilledText"),
    R("GreatestDamageDealtText"),
    R("GreatestDamageTakenText"),
    R("TotalDeathsText"),
    G(false),
    R("GreatestMonsterKilledText"),
    M,
    G(false),
];

/// Row order of `tab2ListBox` (`CharacterSheetPage2::BuildList`).
const TAB2: &[It] = &[
    H("HeaderDPSText"),
    G(true),
    S,
    S,
    G(false),
    S,
    S,
    G(false),
    R("AvgDmgText"),
    S,
    G(false),
    H("HeaderCharacterText"),
    G(true),
    R("AttacksPerSecondText"),
    R("AttackSpeedText"),
    R("CritDmgText"),
    G(false),
    R("RunSpeedText"),
    R("HealIncreaseText"),
    R("LifeRegenText"),
    R("ManaRegenText"),
    R("EnergyAbsorpText"),
    R("ConstitutionText"),
    R("BonusExpGainedText"),
    R("BonusLightRadiusText"),
    G(false),
    R("BonusAllSkillsText"),
    R("BonusSoldierSkillsText"),
    R("BonusDemoSkillsText"),
    R("BonusOccuSkillsText"),
    R("BonusNightbladeSkillsText"),
    R("BonusArcanistSkillsText"),
    R("BonusShamanSkillsText"),
    R("BonusInquisitorSkillsText"),
    R("BonusNecroSkillsText"),
    R("BonusOathkeeperSkillsText"),
    R("BonusBerserkerSkillsText"),
    G(false),
    H("HeaderPhysicalText"),
    G(true),
    R("PhysicalAbsDmgText"),
    R("PhysicalPercentDmgText"),
    G(false),
    R("PierceAbsDmgText"),
    R("PiercePercentDmgText"),
    G(false),
    R("BleedAbsDmgText"),
    R("BleedPercentDmgText"),
    R("BleedDurationText"),
    G(false),
    R("InternalTraumaAbsDmgText"),
    R("InternalTraumaPercentDmgText"),
    R("InternalTraumaDurationText"),
    G(false),
    R("DamageToHealthText"),
    G(false),
    H("HeaderMagicalText"),
    G(true),
    R("CastSpeedText"),
    R("CooldownReductionText"),
    R("ManaCostReductionText"),
    G(false),
    R("PercentHealthDamageText"),
    G(false),
    R("FireAbsDmgText"),
    R("FirePercentDmgText"),
    G(false),
    R("ColdAbsDmgText"),
    R("ColdPercentDmgText"),
    G(false),
    R("LightningAbsDmgText"),
    R("LightningPercentDmgText"),
    G(false),
    R("AcidAbsDmgText"),
    R("AcidPercentDmgText"),
    G(false),
    R("VitalityAbsDmgText"),
    R("VitalityPercentDmgText"),
    G(false),
    R("AetherAbsDmgText"),
    R("AetherPercentDmgText"),
    G(false),
    R("ChaosAbsDmgText"),
    R("ChaosPercentDmgText"),
    G(false),
    R("BurnAbsDmgText"),
    R("BurnPercentDmgText"),
    R("BurnDurationText"),
    G(false),
    R("ColdburnAbsDmgText"),
    R("ColdburnPercentDmgText"),
    R("ColdburnDurationText"),
    G(false),
    R("ElectrocuteAbsDmgText"),
    R("ElectrocutePercentDmgText"),
    R("ElectrocuteDurationText"),
    G(false),
    R("PoisonAbsDmgText"),
    R("PoisonPercentDmgText"),
    R("PoisonDurationText"),
    G(false),
    R("VitalityDecayAbsDmgText"),
    R("VitalityDecayPercentDmgText"),
    R("VitalityDecayDurationText"),
    G(false),
    H("HeaderPetsText"),
    G(true),
    R("PetHealthText"),
    R("PetDamageText"),
    R("PetCritDamageText"),
    R("PetAttackSpeedText"),
    R("PetCastSpeedText"),
    R("PetRunSpeedText"),
    R("PetOAText"),
    R("PetDAText"),
    R("PetPhysicalResistText"),
    R("PetFireResistText"),
    R("PetColdResistText"),
    R("PetLightningResistText"),
    R("PetPoisonResistText"),
    R("PetPierceResistText"),
    R("PetBleedResistText"),
    R("PetVitalityResistText"),
    R("PetAetherResistText"),
    R("PetChaosResistText"),
    R("PetStunResistText"),
    R("PetTrapResistText"),
    R("PetPetrifyResistText"),
    R("PetFreezeResistText"),
    R("PetSleepResistText"),
    R("PetSlowResistText"),
    G(false),
];

/// The zero a row shows without a value (no player yet): the row's format
/// of 0 (`CharacterWindow::UpdatePage2` / `CharacterWindow::UpdatePage3`).
fn zero_text(name: &str) -> &'static str {
    if name.starts_with("Pet")
        || name.contains("Percent")
        || name.ends_with("DurationText")
        || name.contains("Constitution")
        || (name.contains("Bonus") && !name.ends_with("SkillsText"))
        || name.contains("CooldownReduction")
        || name.contains("CritDmg")
        || name.contains("HealIncrease")
    {
        "+0%"
    } else if name.contains("ManaCostReduction") {
        "-0%"
    } else if name.ends_with("ResistText")
        || name.ends_with("SpeedText")
        || name.contains("Absorp")
        || name.contains("Chance")
        || name.contains("Recovery")
        || name.contains("DamageToHealth")
        || name.contains("HealthDamage")
    {
        "0%"
    } else {
        "0"
    }
}

/// A canonical sheet value ("+15%", "-22%", "+2") in the engine's tag
/// format (`tagFormatPlusPercent`, `tagFormatMinusPercent`, `tagFormatPlus`;
/// the tags add their own spacing).
fn tag_value(fonts: &super::text::FontCache, value: String) -> String {
    use gd_sim::items::tooltip::{Arg, format};
    let num = |v: &str| v.parse::<f32>().ok();
    let with = |tag: &str, n: f32| {
        let f = fonts.tag(tag).to_string();
        if f.contains('%') {
            Some(format(&f, &[Arg::Num(n)]))
        } else {
            None
        }
    };
    let out = if let Some(n) = value
        .strip_prefix('+')
        .and_then(|v| v.strip_suffix('%'))
        .and_then(num)
    {
        with("tagFormatPlusPercent", n)
    } else if let Some(n) = value
        .strip_prefix('-')
        .and_then(|v| v.strip_suffix('%'))
        .and_then(num)
    {
        with("tagFormatMinusPercent", n)
    } else if let Some(n) = value.strip_prefix('+').and_then(num) {
        with("tagFormatPlus", n)
    } else {
        None
    };
    out.unwrap_or(value)
}

/// Page I's rows with a rollover (the page's mouse handler
/// `CharacterSheetPage1::HandleMouseEvent`: each tests a row's number and its text / bitmap widget):
/// Health, Energy, Physique, Cunning, Spirit, Offensive / Defensive Ability
/// and the resistances (`tab1<Row>Rollover`; their values: `ui/rollover.rs`).
/// The DPS row's rollover (`tab1DPSRollover`); the armour row's (`tab1CombinedArmorRollover`) is the
/// breakdown window.
fn sheet_rollover(t: &gd_formats::arz::Record, k: &str, r: URect, layout: &mut InvLayout) {
    const ROWS: [&str; 9] = [
        "Life",
        "Mana",
        "Strength",
        "Dexterity",
        "Intelligence",
        "OffensiveAbility",
        "DefensiveAbility",
        "CombinedArmor",
        "DPS",
    ];
    let Some(stem) = k
        .strip_suffix("Bitmap")
        .or_else(|| k.strip_suffix("Number"))
        .or_else(|| k.strip_suffix("Text"))
        .filter(|s| {
            s.starts_with("tab1") && (s.ends_with("Resistance") || ROWS.contains(&&s[4..]))
        })
    else {
        return;
    };
    if let Some(roll) = t.str(&format!("{stem}Rollover")) {
        layout
            .sheet_rollovers
            .push((r, roll.to_owned(), format!("{}Number", &stem[4..])));
    }
}

/// Pages II and III: the expandable list in the page's scroll window
/// (`tabNListBox`: `LineHeight` 19, `NameXPosition` 10 + the row's 7 px
/// padding, collapse icons at `IconXPosition`; the header, spacer and row
/// heights are the builder's 18 / 13 / 10 / 19 px; the scrollbar is the
/// scroll window's `scrollboxwidedark_vscroll`). PORT: the pixel offsets
/// are: header text 30, label 17, value right edge 222, divider y 27
/// from the list's top left.
fn stat_list(
    ui: &mut Ui,
    t: &gd_formats::arz::Record,
    tab: u8,
    origin: Vec2,
    values: &HudValues,
    panels: &super::UiPanels,
    layout: &mut InvLayout,
) {
    let items = if tab == 2 { TAB2 } else { TAB3 };
    let field = |n: &str| format!("tab{tab}{n}");
    let (Some(lb), Some(sw)) = (
        t.str(&field("ListBox")).and_then(|r| ui.record(r)),
        t.str(&field("ScrollWindow")).and_then(|r| ui.record(r)),
    ) else {
        return;
    };
    let record_line = lb.f32("LineHeight").unwrap_or(19.0);
    let view_h = sw.f32("height").unwrap_or(371.0);
    let label_style = lb
        .str("FontStyle")
        .unwrap_or("records/ui/styles/text/style_special_charstats.dbr")
        .to_string();
    let head_style = t.str(&field("HeaderFontStyle")).unwrap_or("").to_string();
    let num_style = t.str(&field("NumberFontStyle")).unwrap_or("").to_string();
    // Row pitch: the record's line height, raised to the taller of the label / number font lines
    // (PORT: stock fixes the pitch to its texture).
    let line = (ui
        .sc(record_line)
        .max(ui.line_height(&label_style))
        .max(ui.line_height(&num_style)))
        / ui.s;
    let divider = t
        .str(&field("HeaderDivider"))
        .unwrap_or("ui/generic/listboxdivider.tex")
        .to_string();
    let view = URect::new(
        origin.x,
        origin.y,
        ui.sc(sw.f32("width").unwrap_or(256.0)),
        ui.sc(view_h),
    );

    enum Placed {
        Head(&'static str, f32, bool),
        Row(&'static str, f32),
        Skill(usize, f32),
        Monster(f32),
    }
    let mut placed = Vec::new();
    let mut y = 5.0f32;
    let (mut collapsed, mut pending_gap, mut skill) = (false, None::<f32>, 0usize);
    for it in items {
        match *it {
            H(n) => {
                if let Some(g) = pending_gap.take() {
                    y += g;
                }
                collapsed = panels.sheet_collapsed.iter().any(|c| *c == field(n));
                placed.push(Placed::Head(n, y, collapsed));
                y += 18.0;
            }
            G(big) => {
                let g = if big { 13.0 } else { 10.0 };
                if collapsed {
                    pending_gap = Some(g);
                } else {
                    y += g;
                }
            }
            R(n) if !collapsed => {
                placed.push(Placed::Row(n, y));
                y += line;
            }
            S if !collapsed => {
                placed.push(Placed::Skill(skill, y));
                skill += 1;
                y += line;
            }
            M if !collapsed => {
                placed.push(Placed::Monster(y));
                y += line;
            }
            _ => {}
        }
    }
    let content_h = y + 4.0;
    let max = (content_h - view_h).max(0.0);
    let scroll = panels.sheet_scroll[tab as usize].clamp(0.0, max);

    let clip = ui
        .commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(view.x * ui.inv),
            top: Val::Px(view.y * ui.inv),
            width: Val::Px(view.w * ui.inv),
            height: Val::Px(view.h * ui.inv),
            overflow: Overflow::clip(),
            ..default()
        })
        .id();
    ui.commands.entity(ui.root).add_child(clip);
    let content = ui
        .commands
        .spawn((
            SheetContent {
                tab,
                max,
                s: ui.s,
                inv: ui.inv,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(-round_px(scroll * ui.s) * ui.inv),
                width: Val::Px(view.w * ui.inv),
                height: Val::Px(ui.sc(content_h) * ui.inv),
                ..default()
            },
        ))
        .id();
    ui.commands.entity(clip).add_child(content);
    let put = |ui: &mut Ui, e: Option<Entity>| {
        if let Some(e) = e {
            ui.commands.entity(content).add_child(e);
        }
    };
    let tbox =
        |x: f32, y: f32, w: f32, h: f32, ax: gd_formats::ui::TextAlign| gd_formats::ui::TextBox {
            style: None,
            rect: gd_formats::ui::Rect::new(x, y, w, h),
            align_x: ax,
            align_y: gd_formats::ui::TextAlign::Center,
            tag: None,
            glow: None,
        };
    use gd_formats::ui::TextAlign as A;
    let mut headers = Vec::new();
    let mut rows = Vec::new();
    // Skill block (page II top): the mouse skills' names and damage per
    // hit (`SheetDps::hits`, `CharacterWindow::UpdatePage2`).
    let skills: Vec<String> = (0..2)
        .filter_map(|n| {
            values
                .stats
                .get(&format!("list.SkillTag{n}"))
                .map(|tag| ui.fonts.tag(tag).to_string())
        })
        .collect();
    // PORT: the label column is the widest label that still leaves the widest value its room;
    // labels and values truncate to their own column, values stay right-aligned.
    let left = ui.sc(17.0);
    let right = ui.sc(222.0).min(view.w);
    let gap = ui.sc(8.0);
    let mut row_texts: Vec<Option<(String, Option<String>)>> = Vec::with_capacity(placed.len());
    let (mut max_label, mut max_value) = (0.0f32, 0.0f32);
    for p in &placed {
        if let Placed::Row(n, _) = p {
            let label = t
                .str(&field(n))
                .map(|tag| ui.fonts.tag(tag).to_string())
                .unwrap_or_default();
            let value = if *n != "AvgDmgText" && *n != "GreatestMonsterKilledText" {
                let key = n.trim_end_matches("Text");
                Some(tag_value(
                    &*ui.fonts,
                    values
                        .stats
                        .get(&format!("list.{key}"))
                        .cloned()
                        .unwrap_or_else(|| zero_text(n).to_string()),
                ))
            } else {
                None
            };
            max_label = max_label.max(ui.text_width(&label_style, &label));
            if let Some(v) = &value {
                max_value = max_value.max(ui.text_width(&num_style, v));
            }
            row_texts.push(Some((label, value)));
        } else {
            row_texts.push(None);
        }
    }
    let label_col = max_label.min((right - left - gap - max_value).max(0.0));
    let value_x = left + label_col + gap;
    let value_w = (right - value_x).max(0.0);
    for (pi, p) in placed.iter().enumerate() {
        match p {
            Placed::Head(n, y, coll) => {
                let ty = ui.sc(*y);
                let icon = if *coll {
                    "ui/generic/listboxcollapsedicon.tex"
                } else {
                    "ui/generic/listboxuncollapsedicon.tex"
                };
                if let Some((h, size)) = ui.image(icon) {
                    let r = URect::new(
                        ui.sc(11.0),
                        ty + ui.sc(2.0),
                        ui.sc(size.x as f32),
                        ui.sc(size.y as f32),
                    );
                    let e = ui.quad(h, r, None, Color::WHITE);
                    put(ui, Some(e));
                }
                if let Some((h, size)) = ui.image(&divider) {
                    let r = URect::new(
                        ui.sc(11.0),
                        ty + ui.sc(22.0),
                        ui.sc(size.x as f32),
                        ui.sc(size.y as f32),
                    );
                    let e = ui.quad(h, r, None, Color::WHITE);
                    put(ui, Some(e));
                }
                let text = t
                    .str(&field(n))
                    .map(|tag| ui.fonts.tag(tag).to_string())
                    .unwrap_or_default();
                let head_w = (ui.sc(230.0).min(view.w) - ui.sc(30.0)).max(0.0);
                let text = ui.truncate_line(&head_style, head_w, &text);
                let e = ui.text_with_style(
                    &tbox(30.0, *y, head_w / ui.s, 24.0, A::Start),
                    &head_style,
                    Vec2::ZERO,
                    &text,
                );
                put(ui, e);
                headers.push((
                    field(n),
                    URect::new(view.x, view.y + ty, view.w, ui.sc(24.0)),
                ));
            }
            Placed::Row(n, y) => {
                // The row's rollover (`tab<N><Row>Rollover`, `CharacterSheetPage::HandleMouseEvent`;
                // PORT: the whole row, clipped to the view).
                let ry = view.y + ui.sc(*y);
                if let Some(roll) =
                    t.str(&field(&format!("{}Rollover", n.trim_end_matches("Text"))))
                {
                    rows.push((
                        URect::new(view.x, ry, ui.sc(222.0), ui.sc(line)),
                        roll.to_owned(),
                        format!("list.{}", n.trim_end_matches("Text")),
                    ));
                }
                let (label, value) = row_texts[pi].as_ref().unwrap();
                let label = ui.truncate_line(&label_style, label_col, label);
                let e = ui.text_with_style(
                    &tbox(17.0, *y, label_col / ui.s, line, A::Start),
                    &label_style,
                    Vec2::ZERO,
                    &label,
                );
                put(ui, e);
                // Label-only rows: the weapon damage heading, the greatest monster (its name is the line under it).
                if let Some(value) = value {
                    let mut tb = tbox(value_x / ui.s, *y, value_w / ui.s, line, A::End);
                    tb.style = Some(num_style.clone());
                    let shown = fit_sheet(ui, &tb, value);
                    let e = ui.text_with_style(&tb, &num_style, Vec2::ZERO, &shown);
                    if let Some(e) = e {
                        ui.commands.entity(e).insert(SheetVital {
                            source: VitalSource::TagStats(
                                format!("list.{}", n.trim_end_matches("Text")),
                                zero_text(n).to_string(),
                            ),
                            tb,
                            po: Vec2::ZERO,
                            shown: value.clone(),
                            s: ui.s,
                            inv: ui.inv,
                        });
                    }
                    put(ui, e);
                }
            }
            Placed::Monster(y) => {
                // `GetGreatestMonsterKilledName`: its tag when known, else
                // `tagNone`, after `GetMonsterColorText(4)` (the quest-monster
                // colour, gameengine.dbr `questMonsterColor`), `CharacterWindow::UpdatePage3`.
                let tag = values
                    .stats
                    .get("list.GreatestMonsterName")
                    .filter(|t| !t.is_empty() && ui.fonts.tags.get(t).is_some())
                    .cloned()
                    .unwrap_or_else(|| "tagNone".into());
                let code = ui
                    .data
                    .db
                    .get("records/game/gameengine.dbr")
                    .and_then(|e| e.str("questMonsterColor").map(str::to_owned))
                    .unwrap_or_default();
                let name = format!("{code}{}", ui.fonts.tag(&tag));
                let mut tb = tbox(17.0, *y, (right - left) / ui.s, line, A::Start);
                tb.style = Some(label_style.clone());
                let shown = fit_sheet(ui, &tb, &name);
                let e = ui.text_with_style(&tb, &label_style, Vec2::ZERO, &shown);
                if let Some(e) = e {
                    ui.commands.entity(e).insert(SheetVital {
                        source: VitalSource::Monster,
                        tb,
                        po: Vec2::ZERO,
                        shown: name,
                        s: ui.s,
                        inv: ui.inv,
                    });
                }
                put(ui, e);
            }
            Placed::Skill(i, y) => {
                // Entries 0 / 2: the mouse skills' names; 4: the weapon damage
                // line under its heading (1 / 3, the skills' damage ranges).
                // A skill's name and damage show its `tab2LMBRollover` /
                // `tab2RMBRollover`. PORT: the row rects.
                let ry = view.y + ui.sc(*y);
                if *i < 4
                    && skills.get(i / 2).is_some()
                    && let Some(roll) =
                        t.str(&field(if *i < 2 { "LMBRollover" } else { "RMBRollover" }))
                {
                    rows.push((
                        URect::new(view.x, ry, ui.sc(222.0), ui.sc(line)),
                        roll.to_owned(),
                        format!("list.Skill{}", i / 2),
                    ));
                }
                if *i < 4
                    && i % 2 == 0
                    && let Some(name) = skills.get(i / 2)
                {
                    let shown = ui.truncate_line(&label_style, right - left, name);
                    let e = ui.text_with_style(
                        &tbox(17.0, *y, (right - left) / ui.s, line, A::Start),
                        &label_style,
                        Vec2::ZERO,
                        &shown,
                    );
                    put(ui, e);
                } else if *i < 4
                    && i % 2 == 1
                    && skills.get(i / 2).is_some()
                    && let Some(v) = values.stats.get(&format!("list.SkillDmg{}", i / 2))
                {
                    let mut tb = tbox(17.0, *y, (right - left) / ui.s, line, A::Start);
                    tb.style = Some(num_style.clone());
                    let shown = fit_sheet(ui, &tb, v);
                    let e = ui.text_with_style(&tb, &num_style, Vec2::ZERO, &shown);
                    if let Some(e) = e {
                        ui.commands.entity(e).insert(SheetVital {
                            source: VitalSource::Stats(
                                format!("list.SkillDmg{}", i / 2),
                                String::new(),
                            ),
                            tb,
                            po: Vec2::ZERO,
                            shown: v.clone(),
                            s: ui.s,
                            inv: ui.inv,
                        });
                    }
                    put(ui, e);
                } else if *i == 4 {
                    let v = values.stats.get("list.AvgDmg").cloned().unwrap_or_default();
                    let mut tb = tbox(17.0, *y, (right - left) / ui.s, line, A::Start);
                    tb.style = Some(num_style.clone());
                    let shown = fit_sheet(ui, &tb, &v);
                    let e = ui.text_with_style(&tb, &num_style, Vec2::ZERO, &shown);
                    if let Some(e) = e {
                        ui.commands.entity(e).insert(SheetVital {
                            source: VitalSource::Stats("list.AvgDmg".into(), String::new()),
                            tb,
                            po: Vec2::ZERO,
                            shown: v,
                            s: ui.s,
                            inv: ui.inv,
                        });
                    }
                    put(ui, e);
                }
            }
        }
    }

    let mut list = SheetList {
        tab,
        view,
        max,
        s: ui.s,
        headers,
        rows,
        ..Default::default()
    };
    if let Some(path) = sw.str("verticalScrollbar") {
        let bx = view.x + ui.sc(240.0);
        let by = view.y - ui.sc(15.0);
        let bh = ui.sc(372.0);
        let w = ui
            .record(path)
            .and_then(|r| r.str("upButton").map(str::to_owned))
            .and_then(|p| ui.record(&p))
            .and_then(|b| b.str("bitmapNameUp").map(str::to_owned))
            .and_then(|p| ui.image(&p))
            .map_or(0.0, |(_, s)| ui.sc(s.x as f32));
        if let Some(bar) = super::window::scrollbar_live(
            ui,
            path,
            bx + w,
            by,
            bh,
            (view_h, content_h, scroll),
            super::window::ScrollOwner::Sheet,
            tab as usize,
        ) {
            list.bar = Some(bar);
        }
    }
    layout.sheet_list = Some(list);
}

/// The scrolled rows of a stats list (page II / III), built at scroll 0 under the list's clip node.
#[derive(Component)]
pub struct SheetContent {
    tab: u8,
    max: f32,
    s: f32,
    inv: f32,
}

/// Scrolls the stats lists in place: moves the content up by `round_px(scroll × s)` and the scroll bar
/// thumb pieces along their track. The hit rectangles ([`SheetList`]) stay at scroll 0 and shift when read.
pub fn scroll_sheet(
    panels: Res<super::UiPanels>,
    mut content: Query<(&SheetContent, &mut Node), Without<super::window::ScrollThumb>>,
    mut thumbs: Query<(&super::window::ScrollThumb, &mut Node), Without<SheetContent>>,
) {
    for (c, mut node) in &mut content {
        let v = panels.sheet_scroll[c.tab as usize].clamp(0.0, c.max);
        let top = Val::Px(-round_px(v * c.s) * c.inv);
        if node.top != top {
            node.top = top;
        }
    }
    for (t, mut node) in &mut thumbs {
        if t.owner != super::window::ScrollOwner::Sheet {
            continue;
        }
        let v = panels.sheet_scroll[t.pane].clamp(0.0, t.max);
        let off = t.travel * (v / t.max).clamp(0.0, 1.0);
        let top = Val::Px(((t.base_y + off).round() + t.round_add) * t.inv);
        if node.top != top {
            node.top = top;
        }
    }
}

/// Money with a comma every three digits (`UI::FormatThousands`, the
/// inventory header's money text: `%i` then `,` before each digit group).
pub(crate) fn money_text(v: u64) -> String {
    let d = v.to_string();
    let n = d.len();
    d.chars().enumerate().fold(String::new(), |mut s, (i, c)| {
        if i > 0 && (n - i) % 3 == 0 {
            s.push(',');
        }
        s.push(c);
        s
    })
}

/// Equipment slot: the silhouette bitmap centred in the item box.
fn item_box(ui: &mut Ui, rect: URect, silhouette: Option<&str>, o: Vec2) {
    let Some((h, size)) = silhouette.and_then(|s| ui.image(s)) else {
        return;
    };
    let (bw, bh) = (ui.sc(rect.w), ui.sc(rect.h));
    let (sw, sh) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
    let r = URect::new(
        o.x + ui.sc(rect.x) + ((bw - sw) * 0.5).trunc(),
        o.y + ui.sc(rect.y) + ((bh - sh) * 0.5).trunc(),
        sw,
        sh,
    );
    ui.quad(h, r, None, Color::WHITE);
}

/// Every record-valued field of a stats-tab master: bitmaps are drawn, static
/// strings show their tag, numbers a placeholder.
fn stats_pane(
    ui: &mut Ui,
    t: &gd_formats::arz::Record,
    po: Vec2,
    values: &HudValues,
    vitals: &super::HudVitals,
    layout: &mut InvLayout,
) {
    let mut fields: Vec<(String, String)> = t
        .fields
        .iter()
        .filter_map(|(k, v)| match v.first() {
            Some(gd_formats::arz::RawValue::Str(s)) if s.ends_with(".dbr") => {
                Some((k.to_string(), s.to_string()))
            }
            _ => None,
        })
        .collect();
    // Only fields the game reads: the records keep an older layout's overlapping rows.
    let prefix: String = fields
        .first()
        .map(|(k, _)| k.chars().take(4).collect())
        .unwrap_or_default();
    let listed = super::tab_fields::USED_TAB_FIELDS
        .iter()
        .filter(|f| f.starts_with(&prefix))
        .count();
    if listed > 1 {
        fields.retain(|(k, _)| super::tab_fields::USED_TAB_FIELDS.contains(&k.as_str()));
    }
    fields.sort_by_key(|(k, _)| !k.ends_with("Bitmap"));
    for (k, rec) in fields {
        if k.ends_with("Bitmap") && k.len() == 10 && k.starts_with("tab") {
            continue;
        }
        let Some(r) = ui.record(&rec) else { continue };
        let tag = r.str("textTag").map(str::to_string);
        match Widget::from_record(&r) {
            Widget::Button {
                up: Some(up), x, y, ..
            } if values.attribute_points > 0 => {
                let which = if k.ends_with("StrengthButton") {
                    0
                } else if k.ends_with("DexterityButton") {
                    1
                } else if k.ends_with("IntelligenceButton") {
                    2
                } else {
                    continue;
                };
                if let Some((_, r)) = ui.bitmap(&up, x, y, po) {
                    ui.record_path_button_sound(r, &rec);
                    layout.attr_buttons.push((which, r));
                }
            }
            Widget::Bitmap { bitmap, x, y } => {
                if let Some((_, br)) = ui.bitmap(&bitmap, x, y, po) {
                    sheet_rollover(t, &k, br, layout);
                }
            }
            Widget::Text(mut tb) => {
                // With points to spend, the attribute label belongs to the
                // + button (ButtonStatic text boxes are relative to the
                // button: label at button x + textBoxX, after the icon).
                if values.attribute_points > 0
                    && let Some(attr) = ["Strength", "Dexterity", "Intelligence"]
                        .iter()
                        .find(|a| k.ends_with(&format!("{a}Text")))
                    && let Some(br) = t
                        .str(&format!("{}{attr}Button", &k[..4]))
                        .and_then(|p| ui.record(p))
                {
                    tb.rect.x = br.f32("bitmapPositionX").unwrap_or(0.0)
                        + br.f32("textBoxX").unwrap_or(tb.rect.x);
                }
                let has_tag = tag.is_some();
                let text = if k.ends_with("UnusedPointsText") {
                    if values.attribute_points == 0 {
                        continue;
                    }
                    let f = tag
                        .as_deref()
                        .map(|t| ui.fonts.tag(t).to_string())
                        .unwrap_or_default();
                    if f.contains('%') {
                        gd_sim::items::tooltip::format(
                            &f,
                            &[gd_sim::items::tooltip::Arg::Num(
                                values.attribute_points as f32,
                            )],
                        )
                    } else {
                        format!("{} {f}", values.attribute_points)
                    }
                } else if k.ends_with("LifeNumber") && !k.ends_with("RegenNumber") {
                    // `CharacterWindow::UpdatePage1`: `itow(GetCurrentLifeInt) " / " itow((int)total)`.
                    vital_text(super::hud::Stat::Health, vitals)
                } else if k.ends_with("ManaNumber") {
                    vital_text(super::hud::Stat::Energy, vitals)
                } else if k.ends_with("CharacterName") {
                    values.name.clone()
                } else if k.ends_with("CharacterLevel") {
                    // "Level 54 Warlock": `tagCharLevelClass` with the class
                    // name (`gd_sim::progress::class_tag`); `tagCharLevel` without one.
                    let class_tag = gd_sim::progress::class_tag(&ui.data.db, &values.skills);
                    let class =
                        (!class_tag.is_empty()).then(|| ui.fonts.tag(&class_tag).to_string());
                    let cname = class.clone().unwrap_or_default();
                    let args = [
                        gd_sim::items::tooltip::Arg::Num(values.level as f32),
                        gd_sim::items::tooltip::Arg::Str(&cname),
                    ];
                    let f = ui
                        .fonts
                        .tag(if class.is_some() {
                            "tagCharLevelClass"
                        } else {
                            "tagCharLevel"
                        })
                        .to_string();
                    gd_sim::items::tooltip::format(&f, &args)
                } else if let Some(tag) = tag {
                    ui.fonts.tag(&tag).to_string()
                } else if let Some(v) = values.stats.get(
                    k.trim_start_matches("tab")
                        .trim_start_matches(|c: char| c.is_ascii_digit()),
                ) {
                    v.clone()
                } else if Widget::template(&r) == "textnumber" {
                    "0".to_string()
                } else {
                    continue;
                };
                let bx = gd_formats::ui::Rect {
                    x: po.x + ui.sc(tb.rect.x),
                    y: po.y + ui.sc(tb.rect.y),
                    w: ui.sc(tb.rect.w),
                    h: ui.sc(tb.rect.h),
                };
                sheet_rollover(t, &k, bx, layout);
                let shown = fit_sheet(ui, &tb, &text);
                let e = ui.text(&tb, po, &shown);
                let stat_key = k
                    .trim_start_matches("tab")
                    .trim_start_matches(|c: char| c.is_ascii_digit())
                    .to_string();
                let source = if k.ends_with("LifeNumber") && !k.ends_with("RegenNumber") {
                    Some(VitalSource::Vital(super::hud::Stat::Health))
                } else if k.ends_with("ManaNumber") {
                    Some(VitalSource::Vital(super::hud::Stat::Energy))
                } else if !has_tag
                    && (values.stats.contains_key(&stat_key)
                        || Widget::template(&r) == "textnumber")
                {
                    Some(VitalSource::Stats(stat_key, text.clone()))
                } else {
                    None
                };
                if let (Some(e), Some(source)) = (e, source) {
                    ui.commands.entity(e).insert(SheetVital {
                        source,
                        tb: tb.clone(),
                        po,
                        shown: text,
                        s: ui.s,
                        inv: ui.inv,
                    });
                }
            }
            _ => {}
        }
    }
}

/// One line of a sheet number cut to its text box width (the box's own style).
fn fit_sheet(ui: &mut Ui, tb: &gd_formats::ui::TextBox, text: &str) -> String {
    match tb.style.as_deref() {
        Some(style) => ui.truncate_line(style, ui.sc(tb.rect.w), text),
        None => text.to_owned(),
    }
}

/// The sheet's "current / max" life or mana number (`CharacterWindow::UpdatePage1`: truncated ints).
fn vital_text(stat: super::hud::Stat, v: &super::HudVitals) -> String {
    let (cur, max) = match stat {
        super::hud::Stat::Health => (v.health, v.health_max),
        _ => (v.energy, v.energy_max),
    };
    format!("{} / {}", cur.trunc() as i32, max.trunc() as i32)
}

/// What redraws a sheet number in place: the life / mana vitals, the money, a
/// `HudValues::stats` row (as-is or through [`tag_value`]), or the greatest monster's name.
#[derive(Clone)]
enum VitalSource {
    Vital(super::hud::Stat),
    Gold,
    /// The stats key and the text shown while it is absent.
    Stats(String, String),
    /// The stats key and the zero text (the list rows, [`tag_value`] applied).
    TagStats(String, String),
    Monster,
}

#[derive(Component)]
pub struct SheetVital {
    source: VitalSource,
    tb: gd_formats::ui::TextBox,
    po: Vec2,
    shown: String,
    /// The build's UI scale and inverse DPI: the window is not rebuilt on a scale change.
    s: f32,
    inv: f32,
}

/// Redraws the sheet's life / mana numbers, money and stats rows when their text changes.
pub fn sheet_vitals(
    mut commands: Commands,
    (vitals, values): (Res<super::HudVitals>, Res<super::HudValues>),
    data: Res<gd_game_core::data::GameData>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    nodes: Query<(Entity, Ref<SheetVital>, &ChildOf)>,
    mut assets: super::UiAssets,
) {
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let (vitals_changed, values_changed) = (vitals.is_changed(), values.is_changed());
    for (e, v, parent) in &nodes {
        let stale = v.is_added()
            || match v.source {
                VitalSource::Vital(_) => vitals_changed,
                _ => values_changed,
            };
        if !stale {
            continue;
        }
        let text = match &v.source {
            VitalSource::Vital(stat) => vital_text(*stat, &vitals),
            VitalSource::Gold => money_text(values.gold),
            VitalSource::Stats(key, fallback) => {
                let t = values.stats.get(key).unwrap_or(fallback);
                if *t == v.shown {
                    continue;
                }
                t.clone()
            }
            VitalSource::TagStats(key, zero) => {
                tag_value(&assets.fonts, values.stats.get(key).unwrap_or(zero).clone())
            }
            VitalSource::Monster => {
                let tag = values
                    .stats
                    .get("list.GreatestMonsterName")
                    .map(String::as_str)
                    .filter(|t| !t.is_empty() && assets.fonts.tags.get(t).is_some())
                    .unwrap_or("tagNone");
                let code = data
                    .db
                    .get("records/game/gameengine.dbr")
                    .and_then(|e| e.str("questMonsterColor"))
                    .unwrap_or_default();
                format!("{code}{}", assets.fonts.tag(tag))
            }
        };
        if text == v.shown {
            continue;
        }
        commands.entity(e).try_despawn();
        let mut ui = assets.ui(
            &mut commands,
            &data,
            parent.parent(),
            v.s,
            v.inv,
            Vec2::new(size.x as f32, size.y as f32),
        );
        // An empty text draws nothing: keep an empty node so the number tracks its key again.
        let shown_text = fit_sheet(&mut ui, &v.tb, &text);
        let n = ui.text(&v.tb, v.po, &shown_text).unwrap_or_else(|| {
            let n = ui.commands.spawn(Node::default()).id();
            ui.commands.entity(parent.parent()).add_child(n);
            n
        });
        ui.commands.entity(n).insert(SheetVital {
            source: v.source.clone(),
            tb: v.tb.clone(),
            po: v.po,
            shown: text,
            s: v.s,
            inv: v.inv,
        });
    }
}
