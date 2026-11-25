//! The whole item tooltip (`Item::GetUIDisplayText` family, Game.dll): the
//! line order, wording (localisation tags) and text classes of each item
//! class. Colours / fonts come from the text class
//! ([`super::tooltip::CLASS_KEYS`], `records/game/gameengine.dbr`).
//!
//! `Item::GetUIDisplayText`: name (`GetUIGameDescription`:
//! class `GameEngine::GetItemTextClass(classification)`, the
//! name or `tagItemNameAndStack`), `itemText` (class 17), `Soulbound` /
//! `Untradeable` (class 18).
//! `ItemEquipment::GetUIDisplayText`: base, `Illusion:`
//! line, attributes (`CreateUIAttributeText`: quality line, base lines, blank,
//! the stores with the racial bonus after the conversions), ascendant block,
//! skill bonuses (`+1 to all skills in Occultist`), skill modifier blocks,
//! the pet bonus ([`pet_bonus_block`]), granted skills, item set block (blank first), the inserted component
//! (`ItemRelic::GetBoundUIDisplayText`) and augment
//! (`ItemEnchantment::GetBoundUIDisplayText`), each after a
//! blank, a blank, the requirements (`GetUIRequirementText`), a blank and
//! `tagShowDetails` (class 35, unless the "always show details" option `detailItemTooltips`).
//! Requirement reductions (`Item::GenerateRequirementText`,
//! [`super::req_reduction`]), the DPS line while comparing
//! (`GetUIDisplayText_DPS`, [`TipCtx::dps_delta`]) and the
//! granted skills' stat lines with the details flag ([`TipCtx::details`]).
//! Not ported: the
//! off-hand weapon's DPS line (`GetUIDisplayText_OffhandDPS`).
//! `ItemRelic::GetUIDisplayText`: name, `itemText`
//! (class 32), the classification line (class 31: `SimpleStringsFormat`
//! of the classification name and `tagComponentComplete`, or
//! `tagComponentNumber[Rare]` while partial), a blank, the bonus lines
//! (class 33), the completion bonus, a blank, requirements and (not when
//! directions are suppressed) a blank and `tagComponentHelp` /
//! `tagComponentError` / `tagComponentCombine`.
//! `ItemArtifactFormula::GetUIDisplayText`: name,
//! description (class 58), blank, `tagCraftingComponentRequired` (59), the
//! reagents `Name (count)` (60/61), blank, `tagCraftingCreate` (69), blank,
//! the crafted item's tooltip with directions suppressed, then
//! `tagFormulaLearn03` when already learned, else (directions not
//! suppressed) a blank and `tagCraftingLearn`.
//! `ItemEnchantment::GetUIDisplayText`,
//! `ItemSet::GetUIDisplayText`, `OneShot_Potion::
//! GetUIDisplayText`, `QuestItem::GetUIDisplayText`
//! (same structure, see the functions below).
use super::db::{Fields, ItemDb};
use super::resolve::{
    Classification, ResolvedItem, Source, StoreKind, is_equipment_class, record_attrs,
    record_attrs_at, resolve,
};
use super::tooltip::{
    Arg, Have, Line, LineKind, attribute_line_rich, blank, format, format_rich, requirement_lines,
    requirement_lines_reduced, stat_lines, stat_lines_ranged_speed, store_rank, tc,
};
use gd_formats::arz::Record;
use gd_formats::save::item::Item;
use gd_formats::text::TagTable;
use gd_formats::vfs::normalize;

/// What the tooltip may depend on besides the item.
#[derive(Clone, Copy)]
pub struct TipCtx<'a> {
    pub db: &'a ItemDb,
    pub tags: &'a TagTable,
    pub have: Have,
    /// The equipped items (item set counts).
    pub equipped: &'a [Item],
    /// `Item::SuppressTextDirections`: the vendor hover hides directions.
    pub suppress_directions: bool,
    /// Whether the player already learned a blueprint record.
    pub formula_known: Option<&'a dyn Fn(&str) -> bool>,
    /// The skill tooltip context (the main player's skills and profile): the
    /// skill modifier blocks' numbers pass through it; `None` shows the
    /// modifier records' raw values.
    pub skills: Option<&'a crate::skill_tooltip::SkillTipCtx<'a>>,
    /// The details flag of `GetUIDisplayText` (its caller computes it, e.g. `QuestRewardItemIcon::GetRolloverText`:
    /// Ctrl held, the gamepad's left trigger, or the option `detailItemTooltips`):
    /// the granted skills' stat lines show and the hint reads
    /// `tagHideDetails`.
    pub details: bool,
    /// The option `detailItemTooltips`: hides the hint line.
    pub details_option: bool,
    /// `GetUIDisplayText_DPS`'s value: the change of the primary skill's DPS
    /// when this weapon replaces the worn one (rounded numbers, see
    /// `gd_game::sheet_dps::compare_dps`); `None` outside a comparison.
    pub dps_delta: Option<f32>,
    /// The off-hand DPS change (`GetUIDisplayText_OffhandDPS`: a one-hander compared with a worn off-hand
    /// weapon), printed right after the main DPS line.
    pub dps_offhand: Option<f32>,
    /// The player's weapon attack animation rate (frame rate / length,
    /// `gd_game::sheet_dps::weapon_anim_rate`): the weapon's base attack speed
    /// line is scaled by it (`CharAttributeVal_BaseAttackSpeed::CreateText`,
    /// [`super::tooltip::attack_speed_line`]); `None` leaves the line out (no
    /// player context).
    pub weapon_anim_rate: Option<f32>,
}

fn tag<'a>(ctx: &'a TipCtx, key: &'a str) -> &'a str {
    if key.is_empty() {
        ""
    } else {
        ctx.tags.get(key).unwrap_or(key)
    }
}

fn line(text: impl Into<String>, class: u8) -> Line {
    Line {
        text: text.into(),
        kind: LineKind::Base,
        class,
        icon: None,
    }
}

fn push_text(out: &mut Vec<Line>, text: &str, class: u8) {
    // PORT: the `^X` colour in effect carries over a `^n` break (real window:
    // the `^w` before `^n(Used in amulets and medals)` makes that line white).
    let mut carry: Option<char> = None;
    for l in text.lines() {
        let full = match carry {
            Some(c) => format!("^{c}{l}"),
            None => l.to_owned(),
        };
        let mut it = l.chars();
        while let Some(c) = it.next() {
            if c == '^'
                && let Some(n) = it.next()
                && n != '^'
            {
                carry = (n != '-').then_some(n);
            }
        }
        out.push(line(full, class));
    }
}

/// A tag's text with the `^n` newline code (the engine's colour letter N has
/// no colour: it breaks the line) expanded.
fn tag_text<'a>(ctx: &'a TipCtx, key: &'a str) -> std::borrow::Cow<'a, str> {
    let t = tag(ctx, key);
    if !t.contains("^n") && !t.contains("^N") {
        return t.into();
    }
    let mut out = String::with_capacity(t.len());
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        if c == '^' && it.peek().is_some_and(|n| n.eq_ignore_ascii_case(&'n')) {
            it.next();
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out.into()
}

/// `GameEngine::GetItemTextClass(classification)` (the map lookup
/// falls back to 3, `ItemNameCommon`; `GameEngine::LoadFromDatabase` fills it
/// with each `gameiteminfo.dbr` block's class: 0..5 -> 3..8, 6 and 15 -> 9,
/// 8 -> 10, 9 -> 12, 10 -> 14, 11 -> 13, 12 -> 15, 13 -> 16, 14 -> 11
/// `ItemNameEnchantment`, the double rares 16 / 17 -> 5 like Rare).
pub fn name_class(c: Classification) -> u8 {
    match c {
        Classification::Common => 3,
        Classification::Magical => 4,
        Classification::Rare => 5,
        Classification::Epic => 6,
        Classification::Legendary => 7,
        Classification::Broken => 8,
        Classification::Potion => 9,
        Classification::Relic => 10,
        Classification::Quest => 12,
        Classification::Artifact => 14,
        Classification::ArtifactFormula => 13,
        Classification::Lore => 16,
        Classification::Enchantment => tc::ENCHANTMENT_NAME_LINE,
    }
}

/// `GameEngine::GetItemClassificationName`: the prefix word of
/// a classification (`tagMagic` ... `tagLegend`; empty for Common and the
/// others).
fn classification_word<'a>(ctx: &'a TipCtx, c: Classification) -> &'a str {
    let key = match c {
        Classification::Magical => "tagMagic",
        Classification::Rare => "tagRare",
        Classification::Epic => "tagEpic",
        Classification::Legendary => "tagLegend",
        _ => return "",
    };
    tag(ctx, key)
}

/// The name text: `tagItemNameOrder` pieces for equipment (markup kept), the
/// record's name tag otherwise; `tagItemNameAndStack` style stack suffix.
fn display_name(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> String {
    let mut name = if is_equipment_class(&r.class) {
        super::naming::compose(ctx.tags, &r.name)
    } else {
        raw_record_name(ctx, base)
    };
    if r.item.stack_count > 1 && r.max_stack > 1 {
        name = format!("{name} ({})", r.item.stack_count);
    }
    name
}

fn raw_record_name(ctx: &TipCtx, rec: &Record) -> String {
    for key in ["itemNameTag", "description"] {
        if let Some(t) = rec.str(key) {
            return tag(ctx, t).to_string();
        }
    }
    String::new()
}

fn name_line(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Line {
    // Both affixes Rare: the double-rare class 16/17, drawn as Rare ([`name_class`]).
    // `ItemRelic/Artifact/ArtifactFormula/Enchantment::Load`: these classes set
    // their own classification (8 / 10 / 11 / 14, see `resolve`), whatever the
    // record's `itemClassification`; PORT: quest items as Quest.
    let c = match r.class.as_str() {
        "ItemRelic" => Classification::Relic,
        "ItemArtifact" => Classification::Artifact,
        "ItemArtifactFormula" => Classification::ArtifactFormula,
        "ItemEnchantment" => Classification::Enchantment,
        "QuestItem" => Classification::Quest,
        _ => super::loot::rarity(r),
    };
    let class = name_class(c);
    Line {
        text: display_name(ctx, r, base),
        kind: LineKind::Name,
        class,
        icon: symbol_bitmap(ctx, r).map(|t| (t, 0.8)),
    }
}

/// The item's symbol: the name line's bitmap at 0.8 x its size (`Item::GetUIGameDescription` puts
/// `ItemEquipment.symbolBitmap` on the name line with scale 0.8; `ItemEquipment::GetUIDisplayText` loads it from
/// `Item::GetSymbolBitmapName`, so only equipment has one). The names are `gameiteminfo.dbr`'s
/// (`GameEngine::LoadFromDatabase`): a pre-awakened item ([`pre_awakened`]) shows `awakenedItemSymbol`; else
/// an item with an ascendant affix (`HasAscendantBonus`) its `<class>AscendedSymbol` by its display class
/// ([`Classification::color_index`]); else a double rare of a rare base (17) `doubleRareMonsterInfrequentSymbol`,
/// a double rare (16) `doubleRareSymbol`, a rare base (`GetItemClassification(0)`, a monster infrequent)
/// `monsterInfrequentSymbol`, else none.
pub fn symbol_bitmap(ctx: &TipCtx, r: &ResolvedItem) -> Option<String> {
    if !crate::items::resolve::is_equipment_class(&r.class) {
        return None;
    }
    let info = ctx.db.get("records/game/gameiteminfo.dbr")?;
    let index = r.display_color_index();
    let field = if pre_awakened(ctx.db, &r.item.base_name) {
        "awakenedItemSymbol".into()
    } else if !r.item.ascendant_name.is_empty() {
        let key = match index {
            0 => "common",
            1 => "magical",
            2 => "rare",
            3 => "epic",
            4 => "legendary",
            16 => "doubleRare",
            17 => "monsterDoubleRare",
            _ => return None,
        };
        format!("{key}AscendedSymbol")
    } else {
        match index {
            17 => "doubleRareMonsterInfrequentSymbol".into(),
            16 => "doubleRareSymbol".into(),
            _ if r.base_classification == Classification::Rare => "monsterInfrequentSymbol".into(),
            _ => return None,
        }
    };
    info.str(&field)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// Whether `base` is a pre-awakened item (`GameEngine::IsPreAwakenedItem`: `GameEngine::LoadFromDatabase`
/// reads `gameascendant.dbr`'s `awakenedRecipes`; `EnableGameEngine` keeps the first `reagentBaseBaseName`
/// (`ItemArtifactFormula::GetReagentBaseName`) of each blueprint that `IsBluePrintValid`: it has reagents and
/// its `artifactName` loads (every stock awakened blueprint has reagents, so only the artifact is checked);
/// `Item::GetSymbolBitmapName` compares its record name with them).
fn pre_awakened(db: &super::ItemDb, base: &str) -> bool {
    let Some(asc) = db.get(crate::ascendant::GAME_ASCENDANT) else {
        return false;
    };
    asc.strs("awakenedRecipes").iter().any(|f| {
        db.get(f).is_some_and(|f| {
            db.get(f.string("artifactName")).is_some()
                && f.strs("reagentBaseBaseName")
                    .first()
                    .is_some_and(|n| n.eq_ignore_ascii_case(base))
        })
    })
}

/// `Item::GetUIDisplayText`.
fn item_head(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let mut out = vec![name_line(ctx, r, base)];
    push_text(
        &mut out,
        &tag_text(ctx, base.string("itemText")),
        tc::DESCRIPTION,
    );
    if base.bool("soulbound") {
        out.push(line(tag(ctx, "tagSoulbound"), tc::BASE_STATS));
    } else if base.bool("untradeable") {
        out.push(line(tag(ctx, "tagUntradeable"), tc::BASE_STATS));
    }
    out
}

/// `GetUIDisplayText_RacialBonus` / `RacialBonus::GetUIDisplayText`: per race of
/// `racialBonusRace` the percent / absolute damage bonus and defence lines
/// (tags `RacialBonus*`, the race's `tagRaceNNNP` plural).
fn racial_lines(ctx: &TipCtx, rec: &Record, class: u8) -> Vec<Line> {
    racial_text(ctx.tags, rec, 0)
        .into_iter()
        .map(|t| line(t, class))
        .collect()
}

/// The lines of [`racial_lines`] at a level index (a skill's values are per
/// level, the level-index argument of `RacialBonus::GetUIDisplayText`).
pub(crate) fn racial_text(tags: &TagTable, rec: &Record, li: usize) -> Vec<String> {
    let races = rec.strs("racialBonusRace");
    let mut out = Vec::new();
    if races.is_empty() {
        return out;
    }
    for (field, key) in [
        ("racialBonusPercentDamage", "RacialBonusPercentDamage"),
        ("racialBonusAbsoluteDamage", "RacialBonusAbsoluteDamage"),
        ("racialBonusPercentDefense", "RacialBonusPercentDefense"),
        ("racialBonusAbsoluteDefense", "RacialBonusAbsoluteDefense"),
    ] {
        let v = crate::stats::level_value(rec, field, li).unwrap_or(0.0);
        if v == 0.0 {
            continue;
        }
        for race in &races {
            // PORT: `GetMonsterRaceTagPlural`'s map is built at start-up; the
            // stock tags are `tagRace004P` for `Race004`.
            let digits: String = race.chars().filter(|c| c.is_ascii_digit()).collect();
            let plural_key = format!("tagRace{digits}P");
            let plural = tags.get(&plural_key).unwrap_or(&plural_key);
            if let Some(f) = tags.get(key) {
                out.push(format_rich(f, &[Arg::Num(v), Arg::Str(plural)]));
            }
        }
    }
    out
}

/// Granted skills: `AttributePak` tail → `ItemSkillAugment::GetUIDisplayText_GrantedSkills`: a
/// blank, `tagTooltipSkillsHeader`, then each skill's text
/// (`GameEngine::GenerateUISkillText`): the name (`skillDisplayName` +
/// `tagItemSkill`, or `tagAutoItemSkill` for an auto-controlled skill), the
/// description (`skillBaseDescription`), `tagAutoToggledSkill` for toggled
/// buffs and, with the details flag, the stat block ([`skill_details`]).
fn skill_lines(ctx: &TipCtx, item: Option<&Item>, skills: &[(String, bool)]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut body = Vec::new();
    for (name, auto) in skills {
        let Some(sk) = ctx.db.get(name) else { continue };
        let disp = tag(ctx, sk.string("skillDisplayName")).to_string();
        if disp.is_empty() {
            continue;
        }
        let suffix = tag(
            ctx,
            if *auto {
                "tagAutoItemSkill"
            } else {
                "tagItemSkill"
            },
        );
        body.push(line(format!("{disp}{suffix}"), tc::SKILL_NAME));
        push_text(
            &mut body,
            &tag_text(ctx, sk.string("skillBaseDescription")),
            tc::SKILL_DESCRIPTION,
        );
        if sk.class == "Skill_BuffSelfToggled" {
            body.push(line(tag(ctx, "tagAutoToggledSkill"), tc::SKILL_DESCRIPTION));
        }
        if let Some(item) = item {
            body.extend(skill_details(ctx, item, name));
        }
    }
    if body.is_empty() {
        return out;
    }
    out.push(blank());
    out.push(line(tag(ctx, "tagTooltipSkillsHeader"), tc::SKILL_HEADING));
    out.extend(body);
    out
}

/// The granted skill's stat block, only with the details flag (
/// `ItemSkillAugment::GetUIDisplayText_GrantedSkills` -> `GameEngine::GenerateUISkillText`: when
/// `Skill::GetCurrentLevel` is not 0, `Skill::CreateUIParameterText`
/// prints `tagCurrentLevel` (class 48) and the skill's stat lines
/// in the item's text class (19), then a blank). PORT: the level is the item's
/// grant level (`gd_sim::skills::granted_level` of its grants), the lines are
/// `skill_tooltip::stat_lines` (it carries the mana-reserve line and the summon's pet block) and the pet bonus block.
fn skill_details(ctx: &TipCtx, item: &Item, skill: &str) -> Vec<Line> {
    let Some(sk) = ctx.skills.filter(|_| ctx.details) else {
        return Vec::new();
    };
    let grants = crate::skills::item_skill_grants(sk.db, std::slice::from_ref(item));
    let level = crate::skills::granted_level(&grants, skill);
    if level == 0 {
        return Vec::new();
    }
    let mut out = vec![
        blank(),
        line(
            crate::skill_tooltip::format(
                tag(ctx, "tagCurrentLevel"),
                &[crate::skill_tooltip::Arg::Num(level as f32)],
            ),
            crate::skill_tooltip::class::SKILL_LEVEL_TITLES,
        ),
    ];
    for l in crate::skill_tooltip::stat_lines(sk, skill, level, tc::BONUSES) {
        push_text(&mut out, &l.text, l.class);
    }
    // `CreateUIParameterText` step 7: the pet bonus block (`petBonusName` of the skill's profile) in the item class.
    if let Some(prof) = crate::skills::profile_record(sk.db, skill) {
        out.extend(pet_bonus_block(ctx, &prof, level, tc::BONUSES));
    }
    out.push(blank());
    out
}

fn item_skills(recs: &[&Record]) -> Vec<(String, bool)> {
    let mut v = Vec::new();
    for r in recs {
        let n = r.string("itemSkillName");
        if !n.is_empty() {
            v.push((
                n.to_string(),
                !r.string("itemSkillAutoController").is_empty(),
            ));
        }
    }
    v
}

/// A pet bonus (`petBonusName` of `rec`) at `level` printed for a caller of text class `caller`
/// (`Skill::CreateUIPetBonusText`): a blank, the title by the bonus's `petTypeNames` count and
/// the bonus's attribute lines ([`crate::skill_tooltip::pet_bonus_lines`]), title / lines in classes
/// 25 / 26 for a caller's 26, 28 / 28 for 28, 24 / 42 for 19, 33 and 41, the caller's own for 81 / 82,
/// else 70 / 71 (`PetBonusTitle` / `PetBonus`). Callers: `ItemEquipment::GetUIDisplayText_PetBonus`
/// (the base record's bonus at level 1, class 71; `InitializeItem` ->
/// `CreateItemPetBonus` reads no affix's), a granted skill's `CreateUIParameterText` and
/// `ItemArtifact::GetUIDisplayText` (class 19). PORT: needs the skill tooltip context (the numbers come
/// from it); without it nothing shows.
fn pet_bonus_block(ctx: &TipCtx, rec: &Record, level: u32, caller: u8) -> Vec<Line> {
    use crate::skill_tooltip::class::PET_BONUS_TITLE;
    let Some(sk) = ctx.skills else {
        return Vec::new();
    };
    let lines = crate::skill_tooltip::pet_bonus_lines(sk, rec, level, false);
    if lines.is_empty() {
        return Vec::new();
    }
    let (title, body) = match caller {
        26 => (25, 26),
        28 => (28, 28),
        19 | 33 | 41 => (24, 42),
        81 | 82 => (caller, caller),
        _ => (PET_BONUS_TITLE, crate::skill_tooltip::class::PET_BONUS),
    };
    let mut out = vec![blank()];
    for l in lines {
        let class = if l.class == PET_BONUS_TITLE {
            title
        } else {
            body
        };
        push_text(&mut out, &l.text, class);
    }
    out
}

/// `WeaponArmor_Shield::GetUIArmorSpecificText` (the
/// `WeaponArmor::CreateUIAttributeText` virtual after the quality line and
/// before the base damage list): when the shield's `defensiveBlockChance` is
/// above 0, `tagShieldBlockInfo` (chance, `defensiveBlock`) and
/// `ShieldBlockRecoveryTime` (`blockRecoveryTime`, two decimals), class 18.
/// (`blockAbsorption` is stored but not printed.)
fn shield_lines(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let chance = base.float("defensiveBlockChance");
    if !r.class.starts_with("WeaponArmor_Shield") || chance <= 0.0 {
        return Vec::new();
    }
    let info = format(
        tag(ctx, "tagShieldBlockInfo"),
        &[Arg::Num(chance), Arg::Num(base.float("defensiveBlock"))],
    );
    let recovery = format(
        tag(ctx, "ShieldBlockRecoveryTime"),
        &[Arg::Num(base.float("blockRecoveryTime"))],
    );
    vec![line(info, tc::BASE_STATS), line(recovery, tc::BASE_STATS)]
}

/// Strips trailing blank lines.
fn trim(out: &mut Vec<Line>) {
    while out.last().is_some_and(|l| l.text.is_empty()) {
        out.pop();
    }
}

/// `tagShowDetails` (class 35) after a blank; the game option `detailItemTooltips` hides it.
fn details_hint(ctx: &TipCtx, out: &mut Vec<Line>) {
    if ctx.details_option {
        return;
    }
    out.push(blank());
    out.push(line(
        tag(
            ctx,
            if ctx.details {
                "tagHideDetails"
            } else {
                "tagShowDetails"
            },
        ),
        tc::DIRECTIONS,
    ));
}

/// All tooltip lines of an item; empty when the record is unknown.
pub fn item_tooltip(ctx: &TipCtx, item: &Item) -> Vec<Line> {
    resolve(ctx.db, item).map_or_else(Vec::new, |r| resolved_tooltip(ctx, item, &r))
}

/// [`item_tooltip`] of `item` resolved as `r`.
fn resolved_tooltip(ctx: &TipCtx, item: &Item, r: &ResolvedItem) -> Vec<Line> {
    let Some(base) = ctx.db.get(&item.base_name) else {
        return Vec::new();
    };
    let mut out = match r.class.as_str() {
        c if is_equipment_class(c) => equipment(ctx, r, base, false),
        "ItemRelic" => relic(ctx, r, base),
        "ItemArtifact" => artifact(ctx, r, base, false),
        "ItemArtifactFormula" => formula(ctx, r, base),
        "ItemEnchantment" => enchantment(ctx, r, base),
        "QuestItem" => quest(ctx, r, base),
        c if c.starts_with("OneShot_Potion") => potion(ctx, r, base),
        _ => generic(ctx, r, base),
    };
    trim(&mut out);
    out
}

/// The lines the vendor search looks through (`Item::GenerateSearchText`).
/// `ItemEquipment::GetSearchText`:
/// equipment gets `Item::GetUIDisplayText` (name, text), the attribute text,
/// the skill / modifier / pet blocks, the set block and the requirements, but
/// not the `Illusion:` line, the inserted component / augment blocks or the
/// `tagShowDetails` hint of the tooltip; other classes use their
/// `GetUIDisplayText` with directions suppressed (`ctx.suppress_directions`
/// is set by the caller).
pub fn item_search_text(ctx: &TipCtx, item: &Item) -> Vec<Line> {
    let Some(r) = resolve(ctx.db, item) else {
        return Vec::new();
    };
    if !is_equipment_class(r.class.as_str()) {
        return resolved_tooltip(ctx, item, &r);
    }
    let Some(base) = ctx.db.get(&item.base_name) else {
        return Vec::new();
    };
    equipment(ctx, &r, base, true)
}

fn generic(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let mut out = item_head(ctx, r, base);
    let mut tail = Vec::new();
    // Directions by class (`tagScrollDirections`, `tagNoteHelp` ...).
    let dir = match r.class.as_str() {
        "OneShot_Scroll" => Some("tagScrollDirections"),
        "ItemNote" => Some("tagNoteHelp"),
        "OneShot_Food" => Some("tagPotionDirections"),
        "ItemUsableSkill"
        | "ItemFactionBooster"
        | "ItemFactionWarrant"
        | "ItemDifficultyUnlock"
        | "ItemDevotionReset"
        | "ItemAttributeReset" => Some("tagItemUseDirections"),
        _ => None,
    };
    if let Some(d) = dir
        && !ctx.suppress_directions
    {
        tail.push(blank());
        tail.push(line(tag(ctx, d), tc::DIRECTIONS));
    }
    out.extend(requirement_lines(ctx.tags, &r.requirements, ctx.have));
    if r.max_stack > 1 && !ctx.suppress_directions {
        tail.push(line(tag(ctx, "tagCombineStacks"), tc::DIRECTIONS));
    }
    out.extend(tail);
    out
}

fn quest(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let mut out = item_head(ctx, r, base);
    // `QuestItem::GetUIDisplayText`: a blank, `tagQuestItem`, and (stackable,
    // directions shown) a blank and `tagCombineStacks`.
    out.push(blank());
    out.push(line(tag(ctx, "tagQuestItem"), tc::DIRECTIONS));
    if r.max_stack > 1 && !ctx.suppress_directions {
        out.push(blank());
        out.push(line(tag(ctx, "tagCombineStacks"), tc::DIRECTIONS));
    }
    out
}

/// `OneShot_Potion::GetUIDisplayText`: a blank, then the bonuses
/// (class 19) in the order attribute points, skill points, health, health
/// over time, energy, experience, iron; a blank and `tagPotionDirections`;
/// `tagCombineStacks`.
fn potion(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let mut out = item_head(ctx, r, base);
    out.push(blank());
    let t = |k: &str| ctx.tags.get(k).unwrap_or("");
    let mut fx: Vec<String> = Vec::new();
    let n = |f: &str| base.float(f);
    if n("bonusAttributePoints") != 0.0 {
        fx.push(format_rich(
            t("tagBonusAttribPoint"),
            &[Arg::Num(n("bonusAttributePoints"))],
        ));
    }
    if n("bonusSkillPoints") != 0.0 {
        fx.push(format_rich(
            t("tagBonusSkillPoint"),
            &[Arg::Num(n("bonusSkillPoints"))],
        ));
    }
    // Pool bonuses: percent and absolute combine into one tag.
    for (pct, abs, tp, ta, tpa) in [
        (
            "bonusLifePercent",
            "bonusLifePoints",
            "tagBonusLifePercent",
            "tagBonusLifeAbs",
            "tagBonusLifePercentAbs",
        ),
        (
            "bonusManaPercent",
            "bonusManaPoints",
            "tagBonusManaPercent",
            "tagBonusManaAbs",
            "tagBonusManaPercentAbs",
        ),
    ] {
        let (p, a) = (n(pct), n(abs));
        let s = if p > 0.0 && a > 0.0 {
            format_rich(t(tpa), &[Arg::Num(p), Arg::Num(a)])
        } else if p > 0.0 {
            format_rich(t(tp), &[Arg::Num(p)])
        } else if a > 0.0 {
            format_rich(t(ta), &[Arg::Num(a)])
        } else {
            String::new()
        };
        if !s.is_empty() {
            fx.push(s);
        }
        if pct == "bonusLifePercent" && n("bonusLifePercentSlow") > 0.0 {
            fx.push(format_rich(
                t("tagBonusLifePercentSlow"),
                &[Arg::Num(n("bonusLifePercentSlow"))],
            ));
        }
    }
    if n("bonusExperiencePoints") != 0.0 {
        fx.push(format_rich(
            t("tagBonusExperience"),
            &[Arg::Num(n("bonusExperiencePoints"))],
        ));
    }
    if n("bonusGoldPoints") != 0.0 {
        fx.push(format_rich(
            t("tagBonusMoney"),
            &[Arg::Num(n("bonusGoldPoints"))],
        ));
    }
    let had = !fx.is_empty();
    for s in fx {
        out.push(line(s, tc::BONUSES));
    }
    if !had {
        out.pop();
    }
    out.extend(requirement_lines(ctx.tags, &r.requirements, ctx.have));
    if !ctx.suppress_directions {
        out.push(blank());
        out.push(line(tag(ctx, "tagPotionDirections"), tc::DIRECTIONS));
        if r.max_stack > 1 {
            out.push(line(tag(ctx, "tagCombineStacks"), tc::DIRECTIONS));
        }
    }
    out
}

/// `Armor::GetUIQualityDescription` / `Weapon::
/// GetUIQualityDescription` (class 66, `ArtifactClass`):
/// `PentaStringFormat` of `tagEnchanted` (augmented), `tagStyleAscended`
/// (ascendant affix), the classification word (`GetItemClassificationName`
/// of the item's classification; both affixes Rare: `tagDoubleRare`), the
/// armour class (`armorClassification` Caster / Light / Heavy →
/// `tagArmorClass01..03`, `Armor::Load`) and the item type
/// (`Item::GetItemTypeTag`: `tagHead` ...); weapons `QuadStringFormat`
/// without the armour class, the type from `Weapon::GetWeaponTypeTag`. Nothing
/// when type, classification and augment word are all empty.
fn quality_line(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Option<Line> {
    let weapon = r.class.starts_with("Weapon");
    let ty = if weapon {
        match super::illusion::weapon_type(&r.class) {
            2 => "tagOneHandedAxe",
            3 => "tagOneHandedSword",
            4 => "tagOneHandedMace",
            5 => "tagOneHandedDagger",
            6 => "tagOneHandedScepter",
            7 => "tagSpear",
            8 => "tagTwoHandedRifle",
            9 => "tagStaff",
            11 => "tagShield",
            12 => "tagOffHand",
            13 => "tagOneHandedPistol",
            15 => "tagTwoHandedAxe",
            16 => "tagTwoHandedSword",
            17 => "tagTwoHandedMace",
            18 => "tagTwoHandedSpear",
            _ => "tagWeaponUnknown",
        }
    } else {
        match super::illusion::item_type(&r.class) {
            1 => "tagHead",
            2 => "tagAmulet",
            3 => "tagChest",
            4 => "tagLegs",
            5 => "tagFeet",
            6 => "tagRing",
            7 => "tagGloves",
            13 => "tagBelt",
            14 => "tagShoulders",
            15 => "tagMedal",
            _ => "tagWeaponUnknown",
        }
    };
    let double_rare = r.prefix_classification == Some(Classification::Rare)
        && r.suffix_classification == Some(Classification::Rare);
    let class_word = if double_rare {
        tag(ctx, "tagDoubleRare")
    } else {
        classification_word(ctx, super::loot::rarity(r))
    };
    let enchanted = if r.item.augment_name.is_empty() {
        ""
    } else {
        tag(ctx, "tagEnchanted")
    };
    let ascended = if r.item.ascendant_name.is_empty() {
        ""
    } else {
        tag(ctx, "tagStyleAscended")
    };
    let ty = tag(ctx, ty);
    if ty.is_empty() && class_word.is_empty() && enchanted.is_empty() {
        return None;
    }
    let armor_class = if weapon {
        ""
    } else {
        match base.string("armorClassification") {
            "Caster" => tag(ctx, "tagArmorClass01"),
            "Light" => tag(ctx, "tagArmorClass02"),
            "Heavy" => tag(ctx, "tagArmorClass03"),
            _ => "",
        }
    };
    // The pieces carry their own trailing spaces ("Epic ", "Caster ").
    let text = format!("{enchanted}{ascended}{class_word}{armor_class}{ty}");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(line(text, tc::ARTIFACT_CLASS))
}

/// The record holding a skill's name (`skills::display_record` through the
/// item database: a pet modifier's / toggle's linked skill).
fn skill_display<'a>(ctx: &TipCtx<'a>, skill: &str) -> Option<&'a Record> {
    let mut r = ctx.db.get(skill)?;
    let first = r;
    for _ in 0..3 {
        if r.str("skillDisplayName").is_some() {
            return Some(r);
        }
        // Walks `buffSkillName` / `petSkillName` links to the record holding a field (not the profile lookup).
        let Some(next) = ["buffSkillName", "petSkillName"]
            .iter()
            .find_map(|k| r.str(k).and_then(|p| ctx.db.get(p)))
        else {
            break;
        };
        r = next;
    }
    Some(if r.str("skillDisplayName").is_some() {
        r
    } else {
        first
    })
}

/// Whether `skill` greys out: the skill bonus lines' and the modifier blocks' grey variant (
/// `ItemSkillAugment::GetUIDisplayText`, `SkillModifierContainer::GetUIDisplayText`): the main player's
/// `SkillManager::FindSkillId` has the skill and the skill its `Skill.masterySkill` names (its mastery) has
/// learned level 0. The rule is the
/// mastery's level, not the skill's own. Every class skill is in the manager (`Player::Load` loads
/// all `skillTree1..80` through `SkillManager::LoadSkills`), so a skill of an unchosen mastery
/// greys. `SkillManager::CreateAndLoadSkill`
/// gives a tree's skills and modifiers the tree's mastery as `Skill.masterySkill` but leaves a mastery's own at
/// 0, so a mastery line (`augmentMasteryName`) never greys; nor do item skills (no tree).
fn unlearned(ctx: &TipCtx, skill: &str) -> bool {
    let Some(sk) = ctx.skills else { return false };
    sk.trees
        .trees
        .iter()
        .find(|t| t.contains(skill))
        .and_then(|t| t.mastery())
        .is_some_and(|m| !m.eq_ignore_ascii_case(skill) && sk.book.level(m) == 0)
}

/// Grey text class 81 (`GrayStats2`) and its neighbours.
const GRAY_STATS2: u8 = 81;
const GRAY_STATS3: u8 = 82;

/// Skill bonus lines (the item pak's skill augment text, `ItemSkillAugment::GetUIDisplayText`, class 19):
/// each `augmentSkillName` as `ItemSkillIncrement` ("+3 to Fire Strike"),
/// then each `augmentMasteryName` as `ItemMasteryIncrement` ("+1 to all skills
/// in Occultist"), then `augmentAllLevel` as `ItemAllSkillIncrement`; a level
/// of 0 shows nothing. A line is grey (class 81,
/// colour tags stripped) when [`unlearned`]. PORT: records base,
/// prefix, suffix, modifier in that order.
fn augment_lines(ctx: &TipCtx, recs: &[&Record]) -> Vec<Line> {
    let (mut skills, mut masteries, mut all) = (Vec::new(), Vec::new(), 0i32);
    for r in recs {
        for n in 1..=5 {
            if let (Some(s), Some(l)) = (
                r.str(&format!("augmentSkillName{n}"))
                    .filter(|s| !s.is_empty()),
                r.i32(&format!("augmentSkillLevel{n}")),
            ) {
                skills.push((s.to_string(), l));
            }
            if let (Some(m), Some(l)) = (
                r.str(&format!("augmentMasteryName{n}"))
                    .filter(|s| !s.is_empty()),
                r.i32(&format!("augmentMasteryLevel{n}")),
            ) {
                masteries.push((m.to_string(), l));
            }
        }
        all += r.i32("augmentAllLevel").unwrap_or(0);
    }
    let name = |skill: &str| {
        skill_display(ctx, skill)
            .map(|d| tag(ctx, d.string("skillDisplayName")).to_string())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for (list, key) in [
        (&skills, "ItemSkillIncrement"),
        (&masteries, "ItemMasteryIncrement"),
    ] {
        for (s, l) in list.iter() {
            if *l == 0 {
                continue;
            }
            let n = name(s);
            if let Some(f) = ctx.tags.get(key) {
                let text = format_rich(f, &[Arg::Num(*l as f32), Arg::Str(&n)]);
                // Grey: `LocalizeStripColorTags`, class 81.
                out.push(if unlearned(ctx, s) {
                    line(gd_formats::text::strip_markup(&text), GRAY_STATS2)
                } else {
                    line(text, tc::BONUSES)
                });
            }
        }
    }
    if all != 0
        && let Some(f) = ctx.tags.get("ItemAllSkillIncrement")
    {
        out.push(line(format_rich(f, &[Arg::Num(all as f32)]), tc::BONUSES));
    }
    out
}

/// A record's unjittered conversions as `ConversionAttribute`s (the form
/// `resolve` gives an item's).
fn record_conversions(rec: &Record) -> Vec<super::resolve::Attr> {
    let mut out = Vec::new();
    for sfx in ["", "2"] {
        let pct = rec.float(&format!("conversionPercentage{sfx}"));
        let in_t = rec.string(&format!("conversionInType{sfx}"));
        if pct <= 0.0 || in_t.is_empty() {
            continue;
        }
        out.push(super::resolve::Attr {
            store: StoreKind::Conversion,
            class: "ConversionAttribute",
            source: Source::Base,
            values: vec![
                (format!("conversionPercentage{sfx}"), pct),
                (format!("conversionInType{sfx}:{in_t}"), 0.0),
                (
                    format!(
                        "conversionOutType{sfx}:{}",
                        rec.string(&format!("conversionOutType{sfx}"))
                    ),
                    0.0,
                ),
            ],
            global: None,
        });
    }
    out
}

/// Skill modifier blocks (the item's modifier container, `SkillModifierContainer::GetUIDisplayText`):
/// `modifiedSkillName{n}` → `modifierSkillName{n}` of the base
/// and affix records, grouped by modified skill in a `std::map` (record name
/// order: Fire Strike's `playerclass02` before `playerclass07`). Per skill: the
/// skill's name (class 37, `ItemSkillName`, drawn after its icon, [`modifier_block_icon`]),
/// then each modifier's attribute text at level 1 (`SkillModifierContainer::GetModifierText`, class 26;
/// grey 81 / 82 when the skill's mastery is unlearned, [`unlearned`]:
/// damage through the main player's profile, `DamageAttributeStore::
/// ProcessText`; the other stores). PORT: the lines come from
/// `skill_tooltip::stat_lines` of the modifier record (its params and stores);
/// the transmuter exclusions of `SkillModifierContainer::IsTransmuterExcluded` are not
/// ported. The numbers are the skill tooltip's (`actor::skill_tooltip_damage`,
/// without the player's conversions): 33 → 397 Fire in the real window.
fn modifier_blocks(ctx: &TipCtx, recs: &[&Record]) -> Vec<Line> {
    let mut map: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for r in recs {
        for n in 1..=8 {
            let (Some(target), Some(m)) = (
                r.str(&format!("modifiedSkillName{n}"))
                    .filter(|s| !s.is_empty()),
                r.str(&format!("modifierSkillName{n}"))
                    .filter(|s| !s.is_empty()),
            ) else {
                continue;
            };
            let e = map.entry(target.to_string()).or_default();
            if !e.iter().any(|x| x.eq_ignore_ascii_case(m)) {
                e.push(m.to_string());
            }
        }
    }
    let mut out = Vec::new();
    for (target, mods) in map {
        let Some(disp) = skill_display(ctx, &target) else {
            continue;
        };
        let name = tag(ctx, disp.string("skillDisplayName")).to_string();
        // Grey (`SkillModifierContainer::GetGrayText`): name class 81, lines 82 without colours.
        let gray = unlearned(ctx, &target);
        let (name_class, body_class) = if gray {
            (GRAY_STATS2, GRAY_STATS3)
        } else {
            (tc::SKILL_NAME, tc::SET_BONUSES)
        };
        let mut body = Vec::new();
        for m in &mods {
            let lines = match ctx.skills {
                Some(sk) => crate::skill_tooltip::stat_lines(sk, m, 1, tc::SET_BONUSES),
                None => {
                    let Some(rec) = ctx.db.get(m) else { continue };
                    let mut attrs = record_attrs_at(rec, Source::Base, 0);
                    attrs.extend(record_conversions(rec));
                    attrs.sort_by_key(store_rank);
                    attrs
                        .iter()
                        .filter_map(|a| attribute_line_rich(ctx.tags, a))
                        .map(|t| crate::skill_tooltip::TextLine {
                            class: tc::SET_BONUSES,
                            text: t,
                            icon: None,
                        })
                        .collect()
                }
            };
            for l in lines {
                let text = if gray {
                    gd_formats::text::strip_markup(&l.text)
                } else {
                    l.text
                };
                push_text(&mut body, &text, body_class);
            }
        }
        if body.is_empty() {
            continue;
        }
        // `SkillModifierContainer::AddBlockSeparator` between two blocks: a " " line of class 1 (`DefaultShort`)
        // unless the last line already is one (`SkillModifierContainer::GetUIDisplayText`).
        if !out.is_empty() && out.last().is_some_and(|l: &Line| l.class != 1) {
            out.push(line(" ", 1));
        }
        let mut name_line = line(name, name_class);
        name_line.icon = modifier_block_icon(ctx, &target).map(|b| (b, 0.65));
        out.push(name_line);
        out.extend(body);
    }
    out
}

/// Before the first skill modifier block (`SkillModifierContainer::GetUIDisplayText`, with blocks to print and
/// lines before them): a blank of class 0 (`GameTextLines::AddBlankLine`: unless the last line already is class 0),
/// unless the last line is the `tagTooltipAscensionHeader` text.
fn blank_before_blocks(ctx: &TipCtx, out: &mut Vec<Line>) {
    let Some(last) = out.last() else { return };
    if last.text == tag(ctx, "tagTooltipAscensionHeader") || last.class == tc::DEFAULT {
        return;
    }
    out.push(blank());
}

/// The skill modifier block's icon (`SkillModifierContainer::GetUIDisplayText` /
/// `SkillModifierContainer::GetBlockIcon`): the name line's `GameTextLine` bitmap is the modified skill's first
/// `skillUpBitmapName` (`SkillProfile.skillUpBitmap` of
/// `GetSkillResource` by the skill object's own record; a `Skill_Modifier`'s parent's), at scale 0.65
/// (`0x3F266666`).
fn modifier_block_icon(ctx: &TipCtx, target: &str) -> Option<String> {
    let rec = ctx.db.get(target)?;
    let rec = match ctx.skills {
        Some(sk) if crate::skills::is_modifier_class(&rec.class) => sk
            .trees
            .parent(target, false)
            .and_then(|p| ctx.db.get(p))
            .unwrap_or(rec),
        _ => rec,
    };
    rec.strs("skillUpBitmapName")
        .first()
        .filter(|b| !b.is_empty())
        .map(|b| b.to_string())
}

/// `search`: the `GetSearchText` subset (see [`item_search_text`]).
fn equipment(ctx: &TipCtx, r: &ResolvedItem, base: &Record, search: bool) -> Vec<Line> {
    let item = &r.item;
    let mut out = item_head(ctx, r, base);
    if let Some(t) = ctx.db.get(&item.transmute_name).filter(|_| !search) {
        // `tagTransmuted` + the illusion's name (class 18).
        out.push(line(
            format!("{}{}", tag(ctx, "tagTransmuted"), raw_record_name(ctx, t)),
            tc::BASE_STATS,
        ));
    }
    // `CreateUIAttributeText` (Armor, Weapon,
    // WeaponArmor): the quality line, the base lines
    // (class 18), a blank, then the damage and conversion lists, the racial
    // bonus, the character, defense, retaliation and skill lists and the
    // requirement reductions (class 19). Shields / off-hands put the blank
    // between the base damage and the base armour (none after the armour);
    // weapons have their DPS line and a second blank after it (see below).
    if let Some(q) = quality_line(ctx, r, base) {
        out.push(q);
    }
    out.extend(shield_lines(ctx, r, base));
    let own = |a: &super::resolve::Attr| {
        !matches!(
            a.source,
            Source::Relic | Source::RelicBonus | Source::Augment | Source::Ascendant
        )
    };
    let early =
        |a: &super::resolve::Attr| matches!(a.store, StoreKind::Damage | StoreKind::Conversion);
    // The details flag prints each attribute's `_Min` / `_Max` range numbers
    // (`Create*RangeText`, [`super::tooltip::range_numbers`]); not in the
    // search text (`ItemEquipment::GetSearchText` passes 0).
    let bounds = (ctx.details && !search)
        .then(|| super::resolve::attr_bounds(ctx.db, item))
        .flatten();
    // The base attack speed line's player context: the uncapped attack speed
    // (`GetPlayerBaseAttackSpeed`) and the weapon animation's rate.
    let speed = ctx
        .skills
        .and_then(|s| s.player)
        .map(|(_, p)| p.speed_caps[1].raw)
        .zip(ctx.weapon_anim_rate);
    let first = stat_lines_ranged_speed(
        ctx.tags,
        r,
        |a| own(a) && early(a),
        true,
        bounds.as_ref(),
        speed,
    );
    let second = stat_lines_ranged_speed(
        ctx.tags,
        r,
        |a| own(a) && !early(a),
        true,
        bounds.as_ref(),
        speed,
    );
    let (mut base_dmg, bonus1): (Vec<Line>, Vec<Line>) =
        first.into_iter().partition(|l| l.class == tc::BASE_STATS);
    let (base_def, bonus2): (Vec<Line>, Vec<Line>) =
        second.into_iter().partition(|l| l.class == tc::BASE_STATS);
    // Weapons: the base attack speed word after the base damage lines
    // (`Weapon::CreateUIAttributeText`), `characterBaseAttackSpeedTag`
    // "CharacterAttackSpeedAverage" -> `tagAttackSpeedAverage` (PORT).
    let weapon_armor = r.class.starts_with("WeaponArmor");
    if r.class.starts_with("Weapon") && !weapon_armor {
        let speed = base.string("characterBaseAttackSpeedTag");
        if let Some(word) = speed.strip_prefix("Character")
            && let Some(t) = ctx.tags.get(&format!("tag{word}"))
        {
            base_dmg.push(line(t, tc::BASE_STATS));
        }
    }
    if weapon_armor {
        out.extend(base_dmg);
        out.push(blank());
        out.extend(base_def);
    } else {
        out.extend(base_dmg);
        out.extend(base_def);
        out.push(blank());
        // `Weapon::CreateUIAttributeText` after that blank: the
        // DPS line (`GetUIDisplayText_DPS`, class 18, only while
        // comparing: its sign colour `^g+` / `^r` / `+` then `DamagePerSecond`)
        // and the blank `GameTextLines::AddBlankLine` adds after a non-blank line.
        if let Some(d) = ctx
            .dps_delta
            .filter(|_| r.class.starts_with("Weapon") && !search)
        {
            let dps_line = |key: &str, v: f32| {
                let sign = if v > 0.0 {
                    "^g+"
                } else if v < 0.0 {
                    "^r"
                } else {
                    "+"
                };
                let text = format_rich(tag(ctx, key), &[Arg::Num(v)]);
                line(format!("{sign}{text}"), tc::BASE_STATS)
            };
            out.push(dps_line("DamagePerSecond", d));
            // The off-hand line follows (`GetUIDisplayText_OffhandDPS`), never for a two-hander.
            if let Some(o) = ctx.dps_offhand {
                out.push(dps_line("DamagePerSecondOffHand", o));
            }
            out.push(blank());
        }
    }
    out.extend(bonus1);
    let prefix = ctx.db.get(&item.prefix_name);
    let suffix = ctx.db.get(&item.suffix_name);
    let modifier = ctx.db.get(&item.modifier_name);
    let recs: Vec<&Record> = [Some(base), prefix, suffix, modifier]
        .into_iter()
        .flatten()
        .collect();
    for rec in &recs {
        out.extend(racial_lines(ctx, rec, tc::BONUSES));
    }
    out.extend(bonus2);
    // Ascendant affix: `tagTooltipAscensionHeader` (class 31) + its lines.
    if let Some(asc) = ctx.db.get(&item.ascendant_name) {
        out.push(line(
            tag(ctx, "tagTooltipAscensionHeader"),
            tc::RELIC_NUMBER,
        ));
        for a in record_attrs(asc, Source::Ascendant) {
            if let Some(t) = attribute_line_rich(ctx.tags, &a) {
                push_text(&mut out, &t, tc::BONUSES);
            }
        }
        out.extend(racial_lines(ctx, asc, tc::BONUSES));
    }
    // `ItemEquipment::GetUIDisplayText` after the attribute text: the skill
    // bonuses, the skill modifier blocks, the pet bonus, the granted
    // skills, the set.
    out.extend(augment_lines(ctx, &recs));
    let blocks = modifier_blocks(ctx, &recs);
    if !blocks.is_empty() {
        blank_before_blocks(ctx, &mut out);
    }
    out.extend(blocks);
    out.extend(pet_bonus_block(
        ctx,
        base,
        1,
        crate::skill_tooltip::class::PET_BONUS,
    ));
    out.extend(skill_lines(ctx, Some(item), &item_skills(&recs)));
    // Item set block.
    if let Some(set) = r.item_set.as_deref().filter(|s| !s.is_empty()) {
        let block = set_lines(ctx, set);
        if !block.is_empty() {
            out.push(blank());
            out.extend(block);
        }
    }
    if let Some(rel) = ctx.db.get(&item.relic_name).filter(|_| !search) {
        out.push(blank());
        out.extend(relic_bound(ctx, rel, item.var1));
    }
    if let Some(aug) = ctx.db.get(&item.augment_name).filter(|_| !search) {
        out.push(blank());
        out.extend(enchantment_bound(ctx, aug, item.unk_augment));
    }
    out.push(blank());
    out.extend(requirement_lines_reduced(
        ctx.tags,
        &r.requirements,
        ctx.have,
        Some(&r.class),
    ));
    if !search {
        details_hint(ctx, &mut out);
    }
    out
}

/// A component's bonus lines (class 33): attributes, racial bonus, granted
/// skills.
fn component_bonus(ctx: &TipCtx, rec: &Record, level_index: usize, class: u8) -> Vec<Line> {
    let mut out = Vec::new();
    // The stores print in creation order (damage, conversion, character,
    // defense, retaliation, skill) like `CreateUIAttributeText`.
    let mut attrs = record_attrs_at(rec, Source::Base, level_index);
    attrs.sort_by_key(store_rank);
    for a in attrs {
        if let Some(t) = attribute_line_rich(ctx.tags, &a) {
            push_text(&mut out, &t, class);
        }
    }
    out.extend(racial_lines(ctx, rec, class));
    out.extend(skill_lines(ctx, None, &item_skills(&[rec])));
    out
}

fn relic_levels(rec: &Record, var1: u32) -> (u32, u32) {
    let need = rec.int("completedRelicLevel").max(1) as u32;
    (var1.max(1), need)
}

/// The class line of a component: `SimpleStringsFormat`(classification word,
/// `tagComponentComplete`) when complete, else `tagComponentNumber[Rare]`.
fn relic_class_line(ctx: &TipCtx, rec: &Record, var1: u32, class: u8) -> Line {
    let (level, need) = relic_levels(rec, var1);
    let word = classification_word(ctx, Classification::parse(rec.string("itemClassification")));
    let text = if level >= need {
        format!("{word}{}", tag(ctx, "tagComponentComplete"))
    } else if word.is_empty() {
        format_rich(
            tag(ctx, "tagComponentNumber"),
            &[
                Arg::Str(tag(ctx, "tagComponentIncomplete")),
                Arg::Num(level as f32),
                Arg::Num(need as f32),
            ],
        )
    } else {
        format_rich(
            tag(ctx, "tagComponentNumberRare"),
            &[
                Arg::Str(word),
                Arg::Str(tag(ctx, "tagComponentIncomplete")),
                Arg::Num(level as f32),
                Arg::Num(need as f32),
            ],
        )
    };
    line(text, class)
}

/// `ItemRelic::GetBoundUIDisplayText` (a component inside an
/// item): name (class 29), the class line (31), bonuses (33). A component has
/// no completion bonus (neither this nor `ItemRelic::GetUIDisplayText`
/// prints one; see [`super::components::roll_bonus`]).
fn relic_bound(ctx: &TipCtx, rec: &Record, var1: u32) -> Vec<Line> {
    let mut out = vec![Line {
        text: raw_record_name(ctx, rec),
        kind: LineKind::Name,
        class: tc::RELIC_NAME,
        icon: None,
    }];
    out.push(relic_class_line(ctx, rec, var1, tc::RELIC_NUMBER));
    out.extend(component_bonus(ctx, rec, 0, tc::RELIC_BONUS));
    out
}

fn relic(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let item = &r.item;
    let mut out = vec![name_line(ctx, r, base)];
    push_text(
        &mut out,
        &tag_text(ctx, base.string("itemText")),
        tc::RELIC_DESCRIPTION,
    );
    out.push(relic_class_line(ctx, base, item.var1, tc::RELIC_NUMBER));
    out.push(blank());
    out.extend(component_bonus(ctx, base, 0, tc::RELIC_BONUS));
    let (level, need) = relic_levels(base, item.var1);
    out.push(blank());
    out.extend(requirement_lines(ctx.tags, &r.requirements, ctx.have));
    if !ctx.suppress_directions {
        out.push(blank());
        let met = ctx.have.level >= r.requirements.level as f32;
        let key = if met && level >= need {
            "tagComponentHelp"
        } else if level >= need {
            "tagComponentError"
        } else {
            "tagComponentCombine"
        };
        out.push(line(tag(ctx, key), tc::DIRECTIONS));
    }
    details_hint(ctx, &mut out);
    out
}

/// `ItemArtifact::GetUIDisplayText`: name,
/// `itemText` (32), the class line(s) (66), a blank, then the relic's own
/// attributes (class 19), skill bonuses, skill modifier blocks, racial bonus,
/// pet bonus and granted skills. With a completion bonus (the save's
/// `relic_bonus`; `ItemArtifact::InitializeItem` picks one from
/// `bonusTableName` when it has none, [`super::components::roll_bonus`]; PORT:
/// an empty one shows none): a
/// blank, `tagDeviceBonus` (class 67) and the bonus record's own blocks the same
/// way, its attributes rolled ([`Source::RelicBonus`], `resolve`); for a
/// blueprint's preview (`preview`: `ItemArtifactFormula::LoadArtifact` sets the
/// flag `ItemArtifact.isPreview`) `tagCraftingRandom` (19) instead; a title left last is dropped.
/// Then a blank, the requirements and the details hint. PORT: the class text
/// (`artifact classes` tags set by `ItemArtifact::Load`) is not read.
fn artifact(ctx: &TipCtx, r: &ResolvedItem, base: &Record, preview: bool) -> Vec<Line> {
    let mut out = vec![name_line(ctx, r, base)];
    push_text(
        &mut out,
        &tag_text(ctx, base.string("itemText")),
        tc::RELIC_DESCRIPTION,
    );
    out.push(blank());
    artifact_blocks(ctx, r, base, Source::Base, &mut out);
    let bonus = if preview {
        let mut rng = gd_rules::Rng(r.item.seed);
        (!super::components::roll_bonus(ctx.db, base, &mut rng).is_empty()).then_some(None)
    } else {
        ctx.db.get(&r.item.relic_bonus).map(Some)
    };
    if let Some(bonus) = bonus {
        out.push(blank());
        out.push(line(tag(ctx, "tagDeviceBonus"), tc::ARTIFACT_BONUS_TITLE));
        match bonus {
            None => out.push(line(tag(ctx, "tagCraftingRandom"), tc::BONUSES)),
            Some(b) => artifact_blocks(ctx, r, b, Source::RelicBonus, &mut out),
        }
        if out
            .last()
            .is_some_and(|l| l.class == tc::ARTIFACT_BONUS_TITLE)
        {
            out.pop();
        }
    }
    out.push(blank());
    out.extend(requirement_lines(ctx.tags, &r.requirements, ctx.have));
    details_hint(ctx, &mut out);
    out
}

/// One record's blocks of a relic tooltip ([`artifact`]): the attributes of
/// `source`, the skill bonuses, modifier blocks, racial bonus, pet bonus (at
/// level 1 in class 19) and granted skills of `rec`.
fn artifact_blocks(
    ctx: &TipCtx,
    r: &ResolvedItem,
    rec: &Record,
    source: Source,
    out: &mut Vec<Line>,
) {
    out.extend(stat_lines(ctx.tags, r, |a| a.source == source));
    out.extend(augment_lines(ctx, &[rec]));
    let blocks = modifier_blocks(ctx, &[rec]);
    if !blocks.is_empty() {
        blank_before_blocks(ctx, out);
    }
    out.extend(blocks);
    out.extend(racial_lines(ctx, rec, tc::BONUSES));
    out.extend(pet_bonus_block(ctx, rec, 1, tc::BONUSES));
    out.extend(skill_lines(ctx, None, &item_skills(&[rec])));
}

/// `ItemEnchantment::GetBoundUIDisplayText`: name (class 43), the
/// classification line (31: word + `tagEnchantmentName`), bonuses (33).
fn enchantment_bound(ctx: &TipCtx, rec: &Record, level: u32) -> Vec<Line> {
    let mut out = vec![Line {
        text: raw_record_name(ctx, rec),
        kind: LineKind::Name,
        class: tc::ENCHANTMENT_NAME,
        icon: None,
    }];
    out.push(line(enchant_class_text(ctx, rec), tc::RELIC_NUMBER));
    let idx = (level as usize).saturating_sub(1);
    out.extend(component_bonus(ctx, rec, idx, tc::RELIC_BONUS));
    out
}

/// `SimpleStringsFormat(classification word, kNameTag)`, or
/// `TripleStringFormat2(faction tag, word, kNameTag)` for a faction augment (
/// `ItemEnchantment::GetUIDisplayText` / `GetBoundUIDisplayText`: `ItemEnchantment.faction`, which
/// `ItemEnchantment::Load` sets from `factionSource` through `FactionPack::
/// GetFactionFromString`; -1000 = none).
fn enchant_class_text(ctx: &TipCtx, rec: &Record) -> String {
    let word = classification_word(ctx, Classification::parse(rec.string("itemClassification")));
    let name = tag(ctx, "tagEnchantmentName");
    match faction_tag(rec.string("factionSource")) {
        None => format!("{word}{name}"),
        Some(f) => format!("{} {word}{name}", tag(ctx, &f)),
    }
}

/// `FactionPack::GetFactionTag` of `FactionPack::GetFactionFromString`:
/// names match case-insensitively (`neutralnpc` = `User1`); the tag is `tagFaction` + the
/// type's name (`tagFactionAlone`, `Invisible`, `GenericMonster`, `Player`, `Survivors`, `Aetherials`,
/// `Cthonians`, `Outlaws`, `Beasts`, `User0`..`User40`); anything else is no faction (-1000).
pub fn faction_tag(name: &str) -> Option<String> {
    const NAMED: [&str; 9] = [
        "Alone",
        "Invisible",
        "GenericMonster",
        "Player",
        "Survivors",
        "Aetherials",
        "Cthonians",
        "Outlaws",
        "Beasts",
    ];
    let low = name.to_ascii_lowercase();
    if low == "neutralnpc" {
        return Some("tagFactionUser1".into());
    }
    if let Some(n) = NAMED.iter().find(|n| n.eq_ignore_ascii_case(name)) {
        return Some(format!("tagFaction{n}"));
    }
    let n: u32 = low.strip_prefix("user")?.parse().ok()?;
    (n <= 40 && low == format!("user{n}")).then(|| format!("tagFactionUser{n}"))
}

fn enchantment(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let mut out = vec![name_line(ctx, r, base)];
    push_text(
        &mut out,
        &tag_text(ctx, base.string("itemText")),
        tc::RELIC_DESCRIPTION,
    );
    if base.bool("soulbound") {
        out.push(line(tag(ctx, "tagSoulbound"), tc::BASE_STATS));
    }
    out.push(line(enchant_class_text(ctx, base), tc::RELIC_NUMBER));
    out.push(blank());
    let idx = (r.item.var1 as usize).saturating_sub(1);
    out.extend(component_bonus(ctx, base, idx, tc::RELIC_BONUS));
    out.push(blank());
    out.extend(requirement_lines(ctx.tags, &r.requirements, ctx.have));
    if !ctx.suppress_directions {
        out.push(blank());
        let met = ctx.have.level >= r.requirements.level as f32;
        out.push(line(
            tag(
                ctx,
                if met {
                    "tagEnchantmentUsable"
                } else {
                    "tagEnchantmentUnusable"
                },
            ),
            tc::DIRECTIONS,
        ));
    }
    details_hint(ctx, &mut out);
    out
}

fn formula(ctx: &TipCtx, r: &ResolvedItem, base: &Record) -> Vec<Line> {
    let mut out = vec![name_line(ctx, r, base)];
    // Description (class 58, colour codes stripped).
    let desc = gd_formats::text::strip_markup(&tag_text(ctx, base.string("itemText")));
    push_text(&mut out, &desc, tc::FORMULA_DESCRIPTION);
    out.push(blank());
    out.push(line(
        tag(ctx, "tagCraftingComponentRequired"),
        tc::FORMULA_REAGENTS,
    ));
    // Reagents: base, then 1..6; equal names add up.
    let mut reagents: Vec<(String, u32, String)> = Vec::new();
    for (names, qty) in std::iter::once((
        "reagentBaseBaseName".to_string(),
        "reagentBaseQuantity".to_string(),
    ))
    .chain((1..=6).map(|i| (format!("reagent{i}BaseName"), format!("reagent{i}Quantity"))))
    {
        let rec_name = base
            .strs(&names)
            .first()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let q = base.int(&qty).max(0) as u32;
        let Some(rr) = ctx.db.get(&rec_name) else {
            continue;
        };
        let name = raw_record_name(ctx, rr);
        if let Some(e) = reagents.iter_mut().find(|e| e.0 == name) {
            e.1 += q;
        } else {
            reagents.push((name, q, rec_name));
        }
    }
    for (name, q, _) in reagents {
        // `ItemArtifactFormula::GetUIDisplayText`: the loop pushes class 60 only.
        out.push(line(
            format!("{} ({q})", gd_formats::text::strip_markup(&name)),
            tc::FORMULA_REAGENT_HAVE,
        ));
    }
    out.push(blank());
    out.push(line(tag(ctx, "tagCraftingCreate"), tc::ARTIFACT_CRAFT));
    // The crafted item (`super::crafting::made_item`: the forced item of a faction blueprint, else
    // `artifactName`; `forcedRelicCompletion` makes a component complete). A loot table left as the made item is
    // not expanded (PORT).
    let target = super::crafting::made_item(base).unwrap_or_default();
    let known = ctx.formula_known.is_some_and(|f| f(&r.item.base_name));
    if let Some(trec) = ctx.db.get(target)
        && !trec.class.starts_with("Loot")
    {
        out.push(blank());
        let var1 = if base.bool("forcedRelicCompletion") {
            trec.int("completedRelicLevel").max(1) as u32
        } else {
            0
        };
        let inner = Item {
            base_name: target.to_string(),
            var1,
            stack_count: 1,
            ..Default::default()
        };
        let sub = TipCtx {
            suppress_directions: true,
            ..*ctx
        };
        // A relic preview's bonus is random (`ItemArtifactFormula::LoadArtifact`, [`artifact`]).
        match resolve(ctx.db, &inner).filter(|ri| ri.class == "ItemArtifact") {
            Some(ri) => {
                let mut lines = artifact(&sub, &ri, trec, true);
                trim(&mut lines);
                out.extend(lines);
            }
            None => out.extend(item_tooltip(&sub, &inner)),
        }
    }
    if known {
        out.push(blank());
        out.push(line(tag(ctx, "tagFormulaLearn03"), tc::ARTIFACT_CRAFT));
    } else if !ctx.suppress_directions {
        out.push(blank());
        out.push(line(tag(ctx, "tagCraftingLearn"), tc::ARTIFACT_CRAFT));
    }
    out
}

/// `ItemSet::GetUIDisplayText`: set name (21), description (22),
/// each member (23 not equipped / 24 equipped), a blank, then per piece count
/// `tagSetItems` with the bonuses that count adds (headers class 24 when
/// reached, else 23; lines 26 / 28). A count whose bonuses equal the previous
/// count's shows nothing. Tiers run to [`super::sets::set_size`] (`setSize`,
/// else the member count); every `setMembers` entry is listed, variants of one
/// piece included, owned by record path.
fn set_lines(ctx: &TipCtx, set: &str) -> Vec<Line> {
    let Some(rec) = ctx.db.get(set) else {
        return Vec::new();
    };
    let mut out = vec![line(tag(ctx, rec.string("setName")), tc::SET_NAME)];
    push_text(
        &mut out,
        &tag_text(ctx, rec.string("setDescription")),
        tc::SET_DESCRIPTION,
    );
    let members = rec.strs("setMembers");
    let equipped: Vec<String> = ctx
        .equipped
        .iter()
        .map(|i| normalize(&i.base_name))
        .collect();
    let mut have = 0;
    for m in &members {
        let on = equipped.contains(&normalize(m));
        have += on as u32;
        let name = ctx
            .db
            .get(m)
            .map(|r| gd_formats::text::strip_markup(&raw_record_name(ctx, r)))
            .unwrap_or_default();
        out.push(line(
            name,
            if on {
                tc::SET_COMPONENT_HAVE
            } else {
                tc::SET_COMPONENT_MISSING
            },
        ));
    }
    out.push(blank());
    let mut prev: Vec<super::resolve::Attr> = Vec::new();
    for level in 1..=super::sets::set_size(rec) {
        let attrs: Vec<_> = record_attrs_at(rec, Source::Base, level - 1);
        let mut body = Vec::new();
        let reached = level as u32 <= have;
        let class = if reached {
            tc::SET_BONUSES
        } else {
            tc::SET_BONUSES_OFF
        };
        // Only what this count adds: an attribute whose value equals the
        // previous count's is not shown again (`ItemSet::
        // GetUIDisplayText`: per-array `v[level] != v[level - 1]`).
        for a in attrs.iter().filter(|a| !prev.contains(a)) {
            if let Some(t) = attribute_line_rich(ctx.tags, a) {
                push_text(&mut body, &t, class);
            }
        }
        prev = attrs;
        if body.is_empty() {
            continue;
        }
        let head = format_rich(
            ctx.tags.get("tagSetItems").unwrap_or("({%d0}) Set"),
            &[Arg::Num(level as f32)],
        );
        out.push(line(
            head,
            if reached {
                tc::SET_COMPONENT_HAVE
            } else {
                tc::SET_COMPONENT_MISSING
            },
        ));
        out.extend(body);
        out.push(blank());
    }
    trim(&mut out);
    out
}
