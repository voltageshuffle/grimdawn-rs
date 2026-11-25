//! Display names. `Armor/Weapon::GetGameDescription` (Game.dll) localises
//! `tagItemNameOrder`
//! (`{%_s0}{%_s1}{%_s2}{%_s3}{%_s4}`) with prefix name, quality tag, style
//! tag, base name tag and suffix name. The `_` flag makes
//! `LocalizationManager::BuildFinalString` (Engine) insert a
//! space before a non-empty parameter when the output is already non-empty;
//! string parameters are tag names resolved through the tag
//! table; a missing tag shows the raw string (`LocalizationManager::
//! GetText` Engine: the tag text itself, prefixed "Tag not
//! found: " only with the `ShowTagErrors` debug switch). Stackable items use
//! `tagItemNameAndStack` when the stack is > 1.
use super::resolve::{NameTags, ResolvedItem};
use gd_formats::text::{TagTable, strip_markup};

fn lookup<'a>(tags: &'a TagTable, tag: &'a str) -> &'a str {
    if tag.is_empty() {
        ""
    } else {
        tags.get(tag).unwrap_or(tag)
    }
}

/// Joins the localised parts in `tagItemNameOrder` order.
pub fn compose(tags: &TagTable, n: &NameTags) -> String {
    let mut out = String::new();
    for t in [&n.prefix, &n.quality, &n.style, &n.base, &n.suffix] {
        let s = lookup(tags, t);
        if s.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(s);
    }
    out
}

/// Plain-text name of a resolved item (markup stripped).
pub fn item_name(tags: &TagTable, item: &ResolvedItem) -> String {
    let base = compose(tags, &item.name);
    let base = strip_markup(&base);
    if item.item.stack_count > 1 && item.max_stack > 1 {
        format!("{base} ({})", item.item.stack_count)
    } else {
        base
    }
}

/// Names for non-equipment records (relics, components, potions, notes):
/// `Item::GetGameDescription` uses the record's `description` tag.
pub fn record_name(tags: &TagTable, rec: &gd_formats::arz::Record) -> String {
    for key in ["itemNameTag", "description"] {
        if let Some(t) = rec.str(key) {
            return strip_markup(lookup(tags, t));
        }
    }
    String::new()
}

/// `GetGameDescription` of a bare record (no affixes), markup stripped: the
/// equipment name (quality, style and base tag, `tagItemNameOrder`), else
/// [`record_name`].
pub fn record_description(tags: &TagTable, rec: &gd_formats::arz::Record) -> String {
    if !super::resolve::is_equipment_class(&rec.class) {
        return record_name(tags, rec);
    }
    let n = NameTags {
        quality: rec.str("itemQualityTag").unwrap_or("").to_string(),
        style: rec.str("itemStyleTag").unwrap_or("").to_string(),
        base: rec.str("itemNameTag").unwrap_or("").to_string(),
        ..Default::default()
    };
    strip_markup(&compose(tags, &n))
}

/// A gold pile's name and its colour (`OneShot_Gold::GetGameDescription` /
/// `GetUIGameDescription`): `tagMoneyFormat` localised with the amount and `name_tag`,
/// spacing kept; only `^n`, line breaks and the outer spaces / braces are removed
/// (`Game::StripTextFormatting`, `Game::TrimTextBraces`). The colour is the format's own
/// colour code (`None`: it has none and the classification's colour shows).
pub fn money_label(tags: &TagTable, name_tag: &str, amount: u32) -> (String, Option<[f32; 4]>) {
    use super::tooltip::{Arg, format_spaced};
    let fmt = tags.get("tagMoneyFormat").unwrap_or("tagMoneyFormat");
    let name = tags.get(name_tag).unwrap_or(name_tag);
    let color = fmt
        .split('^')
        .nth(1)
        .and_then(|s| s.chars().next())
        .and_then(gd_formats::text::color_code);
    (
        format_spaced(fmt, &[Arg::Num(amount as f32), Arg::Str(name)]),
        color,
    )
}
