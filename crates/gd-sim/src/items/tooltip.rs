//! Item tooltip text from the game's format tags.
//!
//! Each attribute class has a display tag (`display_tags.rs`, from Game.dll);
//! ranges use the tag + `R` with (min, max), duration damage is followed by
//! `DamageSingleFormatTime` / `DamageRangeFormatTime`, requirements use
//! classes build their text in code (`DamageBasePhysical[R]` +
//! `DamageBasePierceRatio`, `DefenseAbsorptionProtection`); the weapon's
//! base attack speed (base + the modifier table's affix) is the
//! attacks-per-second line ([`attack_speed_line`], the player's speed and
//! the weapon animation's rate passed in).
//! (`DamageAttributeDur::GetText`, `DamageAttributeAbs::
//! GetText`): duration damage shows the total (per-second value
//! × duration, `DamageSingleFormat` / `DamageRangeFormat`), then the type tag,
//! then `tagPerSecond` for a 1 s duration, else `DamageSingleFormatTime` /
//! `DamageRangeFormatTime`; nothing without both damage and duration. Fixed
//! duration effects (`DamageAttributeDurFixed::GetText`: slows, reductions,
//! OA / DA, fumble) show the value itself, the tag right after it and the
//! `DamageFixed*` time ("30% Slow target for 3 Seconds"); reflexes and
//! influences (stun, freeze, confusion...) pass that time as the tag's `%t0`
//! (`DamageFixed*` / `RetaliationFixed*`). Any chance > 0 prefixes the class's
//! chance tag (`tagChanceOf`, `tagChanceTo` for freeze reflexes).
//! Line order (`Armor` / `Weapon` / `WeaponArmor::CreateUIAttributeText`):
//! each store's attributes write into its text
//! lists in creation order; base lines come first (armour: base protection;
//! weapons: base damage then base attack speed; shields / off-hands: base
//! damage then base protection), then damage, conversion, character,
//! defense, retaliation, skill and requirement-reduction attributes. A
//! store's second and third lists are kept in creation order here.
//! The details (Ctrl) view appends each attribute's `_Min` / `_Max` range
//! numbers to its line ([`range_numbers`], [`stat_lines_ranged`]).
use super::display_tags::DISPLAY_TAGS;
use super::resolve::{Attr, ResolvedItem, StoreKind};
use crate::attributes::AttrFamily;
use gd_formats::text::TagTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Name,
    /// Base stats (damage, armor).
    Base,
    Bonus,
    Requirement {
        met: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub kind: LineKind,
    /// `GameTextClass` (index into [`CLASS_KEYS`]): picks the style, indent.
    pub class: u8,
    /// The `GameTextLine` bitmap drawn before the text: (texture path, scale; `GameTextString::
    /// GetIconWidth` = texture size x scale x UI scale): the skill modifier blocks' skill name and an
    /// equipment's name (its symbol, `tooltip_item::symbol_bitmap`).
    pub icon: Option<(String, f32)>,
}

#[derive(Debug, Clone, Copy)]
pub enum Arg<'a> {
    Num(f32),
    Str(&'a str),
}

/// Formats a tag text: `%[+][.p](f|d|t|s|g)N` arguments and `^c` colour
/// codes (dropped) anywhere; `{`/`}` grouping (kept in raw tag files, removed
/// by the tag loader) is ignored. A `%` not starting a spec is literal, as is
/// a `%[+][.p]` not followed by a conversion letter. Whitespace runs collapse
/// to one space and the ends are trimmed.
pub fn format(fmt: &str, args: &[Arg]) -> String {
    render(fmt, args, false)
}

/// [`format`] keeping the `^X` colour codes (the tooltip renderer colours
/// inline spans; plain callers drop them).
pub fn format_rich(fmt: &str, args: &[Arg]) -> String {
    render(fmt, args, true)
}

/// [`format`] keeping `%.Nf`'s trailing zeros ("x1.50"): the floating combat
/// texts (`CriticalHitFormatMult`, kept as the port showed them; the trimming
/// was only checked on item and skill tooltips).
pub fn format_exact(fmt: &str, args: &[Arg]) -> String {
    expand(fmt, args, false, false, true)
}

/// [`format`] keeping the text's own spacing (a tag's double spaces; only the ends are trimmed): the gold
/// pile's name (`naming::money_label`).
pub fn format_spaced(fmt: &str, args: &[Arg]) -> String {
    let mut s = expand(fmt, args, true, false, false);
    trim_in_place(&mut s);
    s
}

/// [`format`] (`rich` = false) or [`format_rich`].
fn render(fmt: &str, args: &[Arg], rich: bool) -> String {
    expand(fmt, args, true, rich, true)
}

/// Longest `%.p` precision (bad data: an unbounded `%.999999999f` would
/// allocate a gigabyte; the stock tags use at most two decimals).
const MAX_PRECISION: usize = 20;

/// The text being expanded: with `collapse`, whitespace runs are written as one
/// space and the ends are trimmed as the text is written.
struct Text {
    s: String,
    collapse: bool,
    /// A whitespace run after non-whitespace text, written before the next character.
    space: bool,
}

impl Text {
    fn push(&mut self, c: char) {
        if self.collapse && c.is_whitespace() {
            self.space = !self.s.is_empty();
        } else {
            self.flush();
            self.s.push(c);
        }
    }

    /// Writes a pending space; the text's length after it.
    fn flush(&mut self) -> usize {
        if std::mem::take(&mut self.space) {
            self.s.push(' ');
        }
        self.s.len()
    }
}

impl std::fmt::Write for Text {
    fn write_str(&mut self, t: &str) -> std::fmt::Result {
        if self.collapse {
            t.chars().for_each(|c| self.push(c));
        } else {
            self.s.push_str(t);
        }
        Ok(())
    }
}

fn trim_in_place(s: &mut String) {
    s.truncate(s.trim_end().len());
    let lead = s.len() - s.trim_start().len();
    s.drain(..lead);
}

fn expand(fmt: &str, args: &[Arg], trim_zeros: bool, rich: bool, collapse: bool) -> String {
    use std::fmt::Write as _;
    let mut out = Text {
        s: String::with_capacity(fmt.len()),
        collapse,
        space: false,
    };
    let mut b = fmt.char_indices().peekable();
    while let Some((at, x)) = b.next() {
        match x {
            '{' | '}' => {}
            '^' => {
                let c = b.next();
                // Rich text keeps the `^X` colour codes for the renderer.
                if rich && let Some((_, c)) = c {
                    out.push('^');
                    out.push(c);
                }
            }
            '%' => {
                let digit = |&(_, c): &(usize, char)| c.is_ascii_digit();
                let value = |d: char| usize::from(d as u8 - b'0');
                let plus = b.next_if(|&(_, c)| c == '+').is_some();
                let mut prec = 0;
                if b.next_if(|&(_, c)| c == '.').is_some() {
                    while let Some((_, d)) = b.next_if(digit) {
                        prec = (prec * 10 + value(d)).min(MAX_PRECISION);
                    }
                }
                let conv = b.next_if(|&(_, c)| matches!(c, 'f' | 'd' | 't' | 's' | 'g'));
                let Some((_, conv)) = conv else {
                    let end = b.peek().map_or(fmt.len(), |&(i, _)| i);
                    out.write_str(&fmt[at..end]).unwrap();
                    continue;
                };
                // An index past `usize` reads argument 0.
                let mut idx = Some(0usize);
                while let Some((_, d)) = b.next_if(digit) {
                    idx = idx.and_then(|i| i.checked_mul(10)?.checked_add(value(d)));
                }
                let v = match args.get(idx.unwrap_or(0)) {
                    Some(Arg::Str(s)) => {
                        if plus && !s.starts_with('-') {
                            out.push('+');
                        }
                        out.write_str(s).unwrap();
                        continue;
                    }
                    Some(Arg::Num(v)) => *v,
                    None => {
                        if plus {
                            out.push('+');
                        }
                        continue;
                    }
                };
                // Numbers have no whitespace: the value is `out.s[start..]`.
                let start = out.flush();
                match conv {
                    'd' => write!(out, "{}", v as i64),
                    't' | 'g' => write!(out, "{}", Num(v)),
                    _ => write!(out, "{v:.prec$}"),
                }
                .unwrap();
                // As `skill_tooltip::format`:
                // `%+.1f` of 3 shows `+3`, of 4.8 `+4.8` (trailing zeros dropped).
                let n = &out.s[start..];
                if trim_zeros && !matches!(conv, 'd' | 't' | 'g') && n.contains('.') {
                    let kept = n.trim_end_matches('0').trim_end_matches('.').len();
                    out.s.truncate(start + kept);
                }
                if plus && !out.s[start..].starts_with('-') {
                    out.s.insert(start, '+');
                }
            }
            other => out.push(other),
        }
    }
    out.s
}

/// `%t`: whole numbers without decimals, others with one.
struct Num(f32);

impl std::fmt::Display for Num {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let v = self.0;
        if (v - v.round()).abs() < 0.05 {
            write!(f, "{}", v.round() as i64)
        } else {
            write!(f, "{v:.1}")
        }
    }
}

/// The display tag of an attribute class ([`DISPLAY_TAGS`] is sorted by class).
fn class_tag(class: &str) -> Option<&'static str> {
    DISPLAY_TAGS
        .binary_search_by(|(c, _)| (*c).cmp(class))
        .ok()
        .map(|i| DISPLAY_TAGS[i].1)
}

/// One text line for an attribute instance, if it has a display tag.
pub fn attribute_line(tags: &TagTable, a: &Attr) -> Option<String> {
    attr_text(tags, a, None, false)
}

/// [`attribute_line`] keeping the `^X` colour codes ([`format_rich`]).
pub fn attribute_line_rich(tags: &TagTable, a: &Attr) -> Option<String> {
    attr_text(tags, a, None, true)
}

/// The weapon's base attack speed line (Game.dll
/// `CharAttributeVal_BaseAttackSpeed::CreateText`): the
/// attribute's value (the base record's `characterBaseAttackSpeed` plus the
/// modifier table's affix, [`super::resolve`]) plus 1, times the player's
/// base attack speed, times the weapon animation's frame rate over its
/// length; localised with `tagAttackSpeed` ("X.XX Attacks per Second").
/// Nothing when the result is not positive — without a main player
/// `GameEngine::GetPlayerBaseAttackSpeed` is 0 and the engine
/// prints no line.
///
/// `player_speed` is that getter: the player's accumulated `AttackSpeed`
/// value × (1 + its modifier %) × (1 + its multiplier) (the accumulator's
/// three banks at indices 15 / 73 / 131, the port's
/// `Profile::speed_caps[1].raw`); `anim_rate` the weapon animation's `GetFrameRate / GetLength` (the
/// `AnimationSet` subobject `Weapon.animationSet`, its `GetFrameRate` / `GetLength` virtuals; the same ratio as
/// [`crate::dps::weapon_rate`]'s animation duration), 1 without one.
pub fn attack_speed_line(
    tags: &TagTable,
    value: f32,
    player_speed: f32,
    anim_rate: f32,
    rich: bool,
) -> Option<String> {
    let speed = (value + 1.0) * player_speed * anim_rate;
    if speed <= 0.0 {
        return None;
    }
    let f = tags.get("tagAttackSpeed")?;
    Some(render(f, &[Arg::Num(speed)], rich))
}

/// The fixed-duration tag stem of a reflex / influence class (`DamageFixed` /
/// `RetaliationFixed`, the `DamageAttributeReflex` / `DamageAttributeInfluence`
/// constructors and their retaliation subclasses'), `None` for other classes.
fn fixed_time_stem(class: &str) -> Option<&'static str> {
    let (kind, stem) = if let Some(k) = class.strip_prefix("DamageAttribute") {
        (k, "DamageFixed")
    } else {
        (
            class.strip_prefix("RetaliationAttribute")?,
            "RetaliationFixed",
        )
    };
    let influence = kind
        .strip_prefix("Abs_")
        .is_some_and(|k| matches!(k, "Confusion" | "Convert" | "Disruption" | "Fear" | "Taunt"));
    (kind.starts_with("Reflex_") || influence).then_some(stem)
}

/// A `DamageAttributeDurFixed` class (its `GetText` / `CreateRangeNumbers`): the
/// `DamageAttributeDur_*` / `RetaliationAttributeDur_*` effects that are no damage
/// over time (slows, reductions, OA / DA, fumble, damage multiplier).
fn is_dur_fixed(class: &str) -> bool {
    let Some(ty) = class
        .strip_prefix("DamageAttributeDur_")
        .or_else(|| class.strip_prefix("RetaliationAttributeDur_"))
    else {
        return false;
    };
    !matches!(
        ty,
        "Physical"
            | "Bleeding"
            | "Cold"
            | "Fire"
            | "Lightning"
            | "Poison"
            | "Chaos"
            | "Life"
            | "Aether"
            | "LifeLeach"
            | "ManaLeach"
    )
}

fn attr_text(tags: &TagTable, a: &Attr, speed: Option<(f32, f32)>, rich: bool) -> Option<String> {
    let t = |k: &str| tags.get(k);
    // `ConversionAttribute` text (`ConversionAttribute::GetText`): `tagDamageConversion`
    // (percent, `tagConversion<in>`, `tagConversion<out>`), or `tagDamageRemoval`
    // (percent, in) when the out type has no name; nothing at 0 %.
    if a.class == "ConversionAttribute" {
        let pct = a
            .values
            .iter()
            .find(|(k, _)| k.starts_with("conversionPercentage"))
            .map(|x| x.1)?;
        if pct * 0.01 <= 0.0 {
            return None;
        }
        let ty = |p: &str| {
            a.values
                .iter()
                .find_map(|(k, _)| {
                    k.strip_prefix(p)
                        .and_then(|r| r.split_once(':'))
                        .map(|(_, n)| n)
                })
                .filter(|n| !n.is_empty())
                .and_then(|n| t(&format!("tagConversion{n}")))
        };
        let inp = ty("conversionInType")?;
        return Some(match ty("conversionOutType") {
            Some(out) => render(
                t("tagDamageConversion")?,
                &[Arg::Num(pct), Arg::Str(inp), Arg::Str(out)],
                rich,
            ),
            None => render(
                t("tagDamageRemoval")?,
                &[Arg::Num(pct), Arg::Str(inp)],
                rich,
            ),
        });
    }
    // By field: merged stores use a generic class (`CharAttribute`) and one
    // class can cover several fields.
    let (f0, first) = a.values.first().map(|(k, v)| (k.as_str(), *v))?;
    let def = crate::attributes::lookup(f0).or_else(|| crate::attributes::class_def(a.class))?;
    let v = |f: Option<&str>| f.and_then(|f| a.get(f)).unwrap_or(0.0);
    match a.class {
        "DamageAttribute_BasePhysical" => {
            let (min, max) = (v(def.min), v(def.max));
            if max <= 0.0 {
                return None;
            }
            let mut s = if max > min + 0.5 {
                let f = t("DamageBasePhysicalR")?;
                render(f, &[Arg::Num(min), Arg::Num(max)], rich)
            } else {
                render(t("DamageBasePhysical")?, &[Arg::Num(min)], rich)
            };
            let ratio = a.get("offensivePierceRatioMin").unwrap_or(0.0);
            if ratio > 0.0
                && let Some(f) = t("DamageBasePierceRatio")
            {
                s.push('\n');
                s.push_str(&render(f, &[Arg::Num(ratio)], rich));
            }
            return Some(s);
        }
        "DefenseAttribute_BaseProtectionAbsorption" => {
            let p = a
                .values
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("defensiveProtection"))
                .map(|x| x.1)
                .filter(|p| *p > 0.0)?;
            let f = t("DefenseAbsorptionProtection")?;
            return Some(render(f, &[Arg::Num(p)], rich));
        }
        // The weapon's base attack speed: the attacks-per-second line
        // ([`attack_speed_line`]) with the player's speed and the weapon
        // animation's rate; no player context, no line (the engine's
        // `GetPlayerBaseAttackSpeed` is 0 without a main player).
        "CharAttributeVal_BaseAttackSpeed" => {
            return speed.and_then(|(p, r)| attack_speed_line(tags, first, p, r, rich));
        }
        _ => {}
    }
    let tag = class_tag(def.class).or_else(|| class_tag(a.class))?;
    let chance = v(def.chance);
    let mut s = match def.family {
        AttrFamily::OffenseDuration | AttrFamily::RetaliationDuration => {
            let (min, dmin) = (v(def.min), v(def.duration_min));
            let (max, dmax) = (v(def.max).max(min), v(def.duration_max).max(dmin));
            if (min == 0.0 && max == 0.0) || (dmin == 0.0 && dmax == 0.0) {
                return None;
            }
            // `DamageAttributeDurFixed::GetText` (slows, reductions, OA / DA, fumble...): the
            // value itself (`DamageSingleFormat` / `DamageRangeFormat`), the type tag right
            // after it ("30" + "% Slow target"), then `DamageFixedSingleFormatSecond` (1 s) /
            // `DamageFixedSingleFormatTime` / `DamageFixedRangeFormatTime`.
            let fixed = is_dur_fixed(def.class) || is_dur_fixed(a.class);
            let (lo, hi) = if fixed {
                (min, max)
            } else {
                (min * dmin, max * dmax)
            };
            let total = if lo == hi {
                let f = t("DamageSingleFormat").unwrap_or("%.0f0");
                render(f, &[Arg::Num(lo)], rich)
            } else {
                let f = t("DamageRangeFormat").unwrap_or("%.0f0-%.0f1");
                render(f, &[Arg::Num(lo), Arg::Num(hi)], rich)
            };
            let raw_tag = t(tag)?;
            let body = render(raw_tag, &[], rich);
            let time = if fixed {
                let key = if dmin != dmax {
                    "DamageFixedRangeFormatTime"
                } else if dmin == 1.0 {
                    "DamageFixedSingleFormatSecond"
                } else {
                    "DamageFixedSingleFormatTime"
                };
                render(
                    t(key).unwrap_or(""),
                    &[Arg::Num(dmin), Arg::Num(dmax)],
                    rich,
                )
            } else if dmin != dmax {
                let f = t("DamageRangeFormatTime").unwrap_or("");
                render(f, &[Arg::Num(dmin), Arg::Num(dmax)], rich)
            } else if dmin == 1.0 {
                render(t("tagPerSecond").unwrap_or(""), &[], rich)
            } else {
                let f = t("DamageSingleFormatTime").unwrap_or("");
                render(f, &[Arg::Num(dmin)], rich)
            };
            let mut s = total;
            if !fixed || raw_tag.starts_with(char::is_whitespace) {
                s.push(' ');
            }
            s.push_str(&body);
            s.push(' ');
            s.push_str(&time);
            trim_in_place(&mut s);
            s
        }
        // `DamageAttributeDurMod::GetText`: no value but
        // a duration modifier, on a class with a `GetDamageTypeTag` virtual
        // (only `DamageAttributeDurMod_Poison`, "DamageDurationPoison"):
        // `IncreasedDuration` (modifier, type text) instead of the value tag.
        AttrFamily::OffenseDurationModifier | AttrFamily::RetaliationDurationModifier
            if a.class == "DamageAttributeDurMod_Poison"
                && v(def.modifier) <= 0.0
                && v(def.duration_modifier) > 0.0 =>
        {
            render(
                t("IncreasedDuration")?,
                &[
                    Arg::Num(v(def.duration_modifier)),
                    Arg::Str(t("DamageDurationPoison")?),
                ],
                rich,
            )
        }
        _ => {
            let min = if def.min.is_some() { v(def.min) } else { first };
            let max = def
                .max
                .and_then(|f| a.get(f))
                .or_else(|| {
                    def.min
                        .and_then(|f| f.strip_suffix("Min"))
                        .and_then(|b| a.get(&format!("{b}Max")))
                })
                .unwrap_or(0.0);
            if min == 0.0 && max == 0.0 {
                return None;
            }
            // A reflex or influence (`DamageAttributeReflex` / `DamageAttributeInfluence`
            // and their `RetaliationAttribute*` copies: knockdown, stun, freeze, confusion,
            // fear, mind control, disruption...) passes its duration as `%t0` text
            // (`DamageAttributeAbs::GetText`): the constructors' `DamageFixed*` /
            // `RetaliationFixed*` tags, `...SingleFormatSecond` when min == max == 1,
            // `...SingleFormatTime` when min == max, else `...RangeFormatTime`; the
            // chance prefix below applies as to any damage.
            if let Some(stem) = fixed_time_stem(def.class).or_else(|| fixed_time_stem(a.class)) {
                let max = max.max(min);
                let key = if max != min {
                    "RangeFormatTime"
                } else if min == 1.0 {
                    "SingleFormatSecond"
                } else {
                    "SingleFormatTime"
                };
                let f = t(&format!("{stem}{key}")).unwrap_or("");
                let when = render(f, &[Arg::Num(min), Arg::Num(max)], rich);
                render(
                    t(tag)?,
                    &[Arg::Str(&format!(" {}", when.trim_start()))],
                    rich,
                )
            } else {
                // A negative value without a max field is no range (a curse's
                // "-50% Movement Speed").
                let ranged = max > min + 0.5 && max > 0.0;
                match t(&format!("{tag}R")) {
                    Some(fr) if ranged => render(fr, &[Arg::Num(min), Arg::Num(max)], rich),
                    // No range tag: `%t0` takes the `DamageRangeFormat` text.
                    None if ranged => {
                        let f = t("DamageRangeFormat").unwrap_or("%.0f0-%.0f1");
                        let range = render(f, &[Arg::Num(min), Arg::Num(max)], rich);
                        render(t(tag)?, &[Arg::Str(&range)], rich)
                    }
                    _ => render(t(tag)?, &[Arg::Num(min)], rich),
                }
            }
        }
    };
    // `DamageAttributeDurMod::GetText`: after the value text, a
    // positive duration modifier appends `ImprovedTimeFormat`
    // (" with +30% Increased Duration"); the duration-only variant is the
    // `IncreasedDuration` arm above. The Ctrl view does not use the range
    // variant (the `GetRangeText` virtual, `ImprovedTimeFormatR`:
    // no item caller; `Create*RangeText` calls the `CreateRangeNumbers` virtual, see
    // [`range_numbers`]).
    if matches!(
        def.family,
        AttrFamily::OffenseDurationModifier | AttrFamily::RetaliationDurationModifier
    ) && !(a.class == "DamageAttributeDurMod_Poison" && v(def.modifier) <= 0.0)
    {
        let d = v(def.duration_modifier);
        if d > 0.0
            && let Some(f) = t("ImprovedTimeFormat")
        {
            s.push(' ');
            s.push_str(render(f, &[Arg::Num(d)], rich).trim());
        }
    }
    if chance > 0.0 {
        let key = if a.class == "DamageAttributeReflex_Freeze" {
            "tagChanceTo"
        } else {
            "tagChanceOf"
        };
        let prefix = t(key).map_or_else(
            || format!("{}% Chance of", Num(chance)),
            |f| render(f, &[Arg::Num(chance)], rich),
        );
        s = format!("{prefix} {s}");
    }
    Some(s)
}

/// The details (Ctrl) view's range numbers of one attribute: the text the
/// engine appends to the attribute's own line (`ItemEquipment::Create*RangeText`
/// family from `ItemEquipment::CreateDamageRangeText` on, called by `Armor` / `Weapon` /
/// `WeaponArmor::CreateUIAttributeText` with the details flag: per slot of the
/// store's non-global list the rolled attribute's `CreateText`, then the
/// `_Min` attribute's `CreateRangeNumbers(_Max attribute)`, which appends
/// `DamageRangeFormatAppend` (" [min-max]") to the last line of the list).
/// `min` / `max` are the slot's attributes of [`super::resolve::AttrBounds`].
/// per class (`CreateRangeNumbers` / `GetRangeNumbers`):
/// - character (`CharAttribute::CreateRangeNumbers`): the format `CharAttribute.format`
///   (`DamageSingleFormat`, `Float1Format` for life / energy regeneration) of
///   both values, unless both are 0; skill (`SkillAttribute`),
///   defense (`DefenseAttribute_Typical`): `DamageSingleFormat`,
///   same rule; conversion (`ConversionAttribute::CreateRangeNumbers`): the percentages;
/// - flat damage / reflexes (`DamageAttributeAbs`): per store its
///   single format `DamageAttributeAbs.singleFormat` when min == max, else `(` range format
///   `DamageAttributeAbs.rangeFormat` `)`
///   (`DamageSingleFormat` / `DamageRangeFormat`; the reflexes
///   `DamageInfluence*Format`), nothing when all four are 0;
/// - `DamageAttribute_Physical`: as flat damage, then the pierce
///   ratios when either is above 0;
/// - modifiers (`DamageAttributeAbsMod`): always the two modifiers;
///   duration modifiers (`DamageAttributeDurMod`): the modifiers
///   when either is not 0, then the duration modifiers when either is above 0;
/// - DoTs (`DamageAttributeDur`): the totals (per-second value
///   times duration, single or `(range)` per store), then the durations;
///   fixed-value effects (`DamageAttributeDurFixed`: slows,
///   reductions, OA / DA, fumble...): the values when any is not 0, then the
///   durations;
/// - base damage / armour / attack speed (`...AbsBase`, `_BasePhysical`,
///   `_BaseProtectionAbsorption`, `CharAttributeVal_BaseAttackSpeed`) and the
///   mana burn defenses: an empty `CreateRangeNumbers`.
///
/// `rich` keeps the `^X` colour codes ([`format_rich`]).
pub fn range_numbers(tags: &TagTable, min: &Attr, max: &Attr, rich: bool) -> Option<String> {
    fn t<'a>(tags: &'a TagTable, k: &'a str) -> &'a str {
        tags.get(k).unwrap_or(k)
    }
    let t = |k: &'static str| t(tags, k);
    let single = |f: &'static str, v: f32| render(t(f), &[Arg::Num(v)], rich);
    let append = |a: &str, b: &str| {
        render(
            t("DamageRangeFormatAppend"),
            &[Arg::Str(a), Arg::Str(b)],
            rich,
        )
    };
    // `(lo-hi)` or the single value, per store.
    let span = |sf: &'static str, rf: &'static str, lo: f32, hi: f32| {
        if lo == hi {
            single(sf, lo)
        } else {
            format!("({})", render(t(rf), &[Arg::Num(lo), Arg::Num(hi)], rich))
        }
    };
    let join = |parts: Vec<String>| {
        let parts: Vec<String> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        (!parts.is_empty()).then(|| format!(" {}", parts.join(" ")))
    };
    let pair = |f: &'static str, a: f32, b: f32| append(&single(f, a), &single(f, b));
    let first = |a: &Attr| a.values.first().map_or(0.0, |(_, v)| *v);
    let class = min.class;
    match class {
        "CharAttributeVal_BaseAttackSpeed"
        | "DamageAttribute_BasePhysical"
        | "DefenseAttribute_BaseProtectionAbsorption"
        | "DefenseAttribute_SkillProtectionAbsorption"
        | "DefenseAttributeAbs_ManaBurn"
        | "DefenseAttributeAbs_ManaBurnRatio" => return None,
        c if c.starts_with("DamageAttributeAbsBase") => return None,
        "CharAttribute" | "SkillAttribute" => {
            let (a, b) = (first(min), first(max));
            if a == 0.0 && b == 0.0 {
                return None;
            }
            let regen = min.values.first().is_some_and(|(k, _)| {
                k.eq_ignore_ascii_case("characterLifeRegen")
                    || k.eq_ignore_ascii_case("characterManaRegen")
            });
            let f = if regen {
                "Float1Format"
            } else {
                "DamageSingleFormat"
            };
            return join(vec![pair(f, a, b)]);
        }
        "ConversionAttribute" => {
            let pct = |a: &Attr| {
                a.values
                    .iter()
                    .find(|(k, _)| k.starts_with("conversionPercentage"))
                    .map_or(0.0, |x| x.1)
            };
            let (a, b) = (pct(min), pct(max));
            if a == 0.0 && b == 0.0 {
                return None;
            }
            return join(vec![pair("DamageSingleFormat", a, b)]);
        }
        _ => {}
    }
    let def = crate::attributes::class_def(class)?;
    let g = |a: &Attr, f: Option<&str>| f.and_then(|f| a.get(f)).unwrap_or(0.0);
    if class.starts_with("DefenseAttribute") {
        let (a, b) = (g(min, def.value), g(max, def.value));
        if a == 0.0 && b == 0.0 {
            return None;
        }
        return join(vec![pair("DamageSingleFormat", a, b)]);
    }
    match () {
        _ if class.contains("AbsMod_") => join(vec![pair(
            "DamageSingleFormat",
            g(min, def.modifier),
            g(max, def.modifier),
        )]),
        _ if class.contains("DurMod_") => {
            let (m0, m1) = (g(min, def.modifier), g(max, def.modifier));
            let (d0, d1) = (g(min, def.duration_modifier), g(max, def.duration_modifier));
            let mut parts = Vec::new();
            if m0 != 0.0 || m1 != 0.0 {
                parts.push(pair("DamageSingleFormat", m0, m1));
            }
            if d0 > 0.0 || d1 > 0.0 {
                parts.push(pair("DamageSingleFormat", d0, d1));
            }
            join(parts)
        }
        _ if class.contains("AttributeDur_") => {
            let v = |a: &Attr| {
                (
                    g(a, def.min),
                    g(a, def.max),
                    g(a, def.duration_min),
                    g(a, def.duration_max),
                )
            };
            let ((a0, a1, ad0, ad1), (b0, b1, bd0, bd1)) = (v(min), v(max));
            let (sf, rf) = ("DamageSingleFormat", "DamageRangeFormat");
            let mut parts = Vec::new();
            if is_dur_fixed(class) {
                if a0 != 0.0 || a1 != 0.0 || b0 != 0.0 || b1 != 0.0 {
                    parts.push(append(&span(sf, rf, a0, a1), &span(sf, rf, b0, b1)));
                }
            } else {
                parts.push(append(
                    &span(sf, rf, ad0 * a0, ad1 * a1),
                    &span(sf, rf, bd0 * b0, bd1 * b1),
                ));
            }
            parts.push(append(&span(sf, rf, ad0, ad1), &span(sf, rf, bd0, bd1)));
            join(parts)
        }
        _ => {
            let physical = class == "DamageAttribute_Physical";
            let (lo, hi) = if physical {
                (Some("offensivePhysicalMin"), Some("offensivePhysicalMax"))
            } else {
                (def.min, def.max)
            };
            let (a0, a1, b0, b1) = (g(min, lo), g(min, hi), g(max, lo), g(max, hi));
            if a0 == 0.0 && a1 == 0.0 && b0 == 0.0 && b1 == 0.0 {
                return None;
            }
            let reflex = class.contains("AttributeReflex_");
            let (sf, rf) = if reflex {
                ("DamageInfluenceSingleFormat", "DamageInfluenceRangeFormat")
            } else {
                ("DamageSingleFormat", "DamageRangeFormat")
            };
            let mut parts = vec![append(&span(sf, rf, a0, a1), &span(sf, rf, b0, b1))];
            if physical {
                let (r0, r1) = (
                    g(min, Some("offensivePierceRatio")),
                    g(max, Some("offensivePierceRatio")),
                );
                if r0 > 0.0 || r1 > 0.0 {
                    parts.push(pair("DamageSingleFormat", r0, r1));
                }
            }
            join(parts)
        }
    }
}

/// Requirement values the character has: level, Physique, Cunning, Spirit.
#[derive(Debug, Clone, Copy, Default)]
pub struct Have {
    pub level: f32,
    pub physique: f32,
    pub cunning: f32,
    pub spirit: f32,
    /// The character's requirement reductions ([`super::req_reduction`]).
    pub reduction: super::req_reduction::Reduction,
}

/// `GameTextClass` values (the order of `GameEngine::LoadGameTextStyles`:
/// `Default` = 0 and each following key's `...Indent`
/// read in sequence; `ItemNameCommon` = 3 matches `GetItemTextClass`'s
/// fallback). The key is the `records/game/gameengine.dbr` field holding the
/// style record (`{key}Indent` the indent).
pub const CLASS_KEYS: [&str; 85] = [
    "Default",
    "DefaultShort",
    "ItemBanner",
    "ItemNameCommon",
    "ItemNameMagical",
    "ItemNameRare",
    "ItemNameEpic",
    "ItemNameLegendary",
    "ItemNameBroken",
    "ItemNamePotion",
    "ItemNameRelic",
    "ItemNameEnchantment",
    "ItemNameQuest",
    "ItemNameArtifactFormula",
    "ItemNameArtifact",
    "ItemNameScroll",
    "ItemNameLore",
    "ItemDescription",
    "ItemBaseStats",
    "ItemBonuses",
    "ItemRequirements",
    "ItemSetName",
    "ItemSetDescription",
    "ItemSetComponentNotAvailable",
    "ItemSetComponentAvailable",
    "ItemSetComponentTitle",
    "ItemSetBonuses",
    "ItemSetBonusesSkill",
    "ItemSetBonusesNotAvailable",
    "ItemRelicName",
    "ItemRelicNameDisabled",
    "ItemRelicNumber",
    "ItemRelicDescription",
    "ItemRelicBonus",
    "ItemRelicCompleteTitle",
    "ItemDirections",
    "ItemSkillHeading",
    "ItemSkillName",
    "ItemSkillNameSet",
    "ItemSkillDescription",
    "ItemSkillDescriptionSet",
    "ItemSkillStats",
    "ItemPetSkillStats",
    "ItemEnchantmentName",
    "ItemEnchantmentStats",
    "ItemEnchantmentStatsNotAvailable",
    "SkillName",
    "SkillDescription",
    "SkillLevelTitles",
    "SkillStatsCurrent",
    "SkillStatsNext",
    "SkillRequirements",
    "SkillRequirementsNotMet",
    "PetSkillNameCurrent",
    "PetSkillNameNext",
    "PetSkillStatsCurrent",
    "PetSkillStatsNext",
    "ArtifactFormulaTitle",
    "ArtifactFormulaDescription",
    "ArtifactFormulaReagents",
    "ArtifactFormulaReagentAvailable",
    "ArtifactFormulaReagentNotAvailable",
    "ArtifactFormulaCostAvailable",
    "ArtifactFormulaCostNotAvailable",
    "ArtifactTitle",
    "ArtifactDescription",
    "ArtifactClass",
    "ArtifactBonusTitle",
    "ArtifactBonus",
    "ArtifactCraft",
    "PetBonusTitle",
    "PetBonus",
    "PetBonusNextTitle",
    "PetBonusNext",
    "EffectHeading",
    "EffectName",
    "EffectDescription",
    "EffectStatsCurrent",
    "EffectStatsNext",
    "Debug",
    "GrayStats1",
    "GrayStats2",
    "GrayStats3",
    "GrayStats4",
    "ItemUpgrade",
];

/// Text class numbers used by the item tooltips.
pub mod tc {
    pub const DEFAULT: u8 = 0;
    pub const ITEM_BANNER: u8 = 2;
    pub const DESCRIPTION: u8 = 17;
    pub const BASE_STATS: u8 = 18;
    pub const BONUSES: u8 = 19;
    pub const REQUIREMENTS: u8 = 20;
    pub const SET_NAME: u8 = 21;
    pub const SET_DESCRIPTION: u8 = 22;
    pub const SET_COMPONENT_MISSING: u8 = 23;
    pub const SET_COMPONENT_HAVE: u8 = 24;
    pub const SET_COMPONENT_TITLE: u8 = 25;
    pub const SET_BONUSES: u8 = 26;
    pub const SET_BONUSES_SKILL: u8 = 27;
    pub const SET_BONUSES_OFF: u8 = 28;
    pub const RELIC_NAME: u8 = 29;
    pub const RELIC_NUMBER: u8 = 31;
    pub const RELIC_DESCRIPTION: u8 = 32;
    pub const RELIC_BONUS: u8 = 33;
    pub const RELIC_COMPLETE_TITLE: u8 = 34;
    pub const DIRECTIONS: u8 = 35;
    pub const SKILL_HEADING: u8 = 36;
    pub const SKILL_NAME: u8 = 37;
    pub const SKILL_DESCRIPTION: u8 = 39;
    pub const ENCHANTMENT_NAME: u8 = 43;
    /// `ItemNameEnchantment`.
    pub const ENCHANTMENT_NAME_LINE: u8 = 11;
    pub const FORMULA_TITLE: u8 = 57;
    pub const FORMULA_DESCRIPTION: u8 = 58;
    pub const FORMULA_REAGENTS: u8 = 59;
    pub const FORMULA_REAGENT_HAVE: u8 = 60;
    pub const FORMULA_REAGENT_MISSING: u8 = 61;
    pub const ARTIFACT_CLASS: u8 = 66;
    pub const ARTIFACT_BONUS_TITLE: u8 = 67;
    pub const ARTIFACT_CRAFT: u8 = 69;
}

/// A blank spacer line (`GameTextLines::AddBlankLine` pushes an empty line).
pub fn blank() -> Line {
    Line {
        text: String::new(),
        kind: LineKind::Base,
        class: tc::DEFAULT,
        icon: None,
    }
}

/// Requirement lines (`ItemEquipment::GetUIRequirementText` →
/// `Item::GenerateRequirementText`, class 20): Player Level,
/// Physique, Cunning, Spirit; a requirement of 0 shows nothing.
pub fn requirement_lines(
    tags: &TagTable,
    r: &super::resolve::Requirements,
    have: Have,
) -> Vec<Line> {
    requirement_lines_reduced(tags, r, have, None)
}

/// [`requirement_lines`] of equipment of `class`:
/// `ItemEquipment::GetUIRequirementText` passes the character's
/// reduction (level: attribute 54, the others `Reduction::for_class`) to
/// `Item::GenerateRequirementText`: met without it = `MeetsRequirement`;
/// met with it = `MeetsRequirementWithReduction` (label, reduced need, need,
/// percent); else `FailsRequirementWithReduction` when the percent is above 0,
/// else `FailsRequirement`.
pub fn requirement_lines_reduced(
    tags: &TagTable,
    r: &super::resolve::Requirements,
    have: Have,
    class: Option<&str>,
) -> Vec<Line> {
    let mut out = Vec::new();
    let pct = class.map_or([0.0; 3], |c| have.reduction.for_class(c));
    let level_pct = if class.is_some() {
        have.reduction.level()
    } else {
        0.0
    };
    for (need, have, label, pct) in [
        (
            r.level as f32,
            have.level,
            tags.get("LevelRequirement").unwrap_or("Player Level"),
            level_pct,
        ),
        (
            r.physique as f32,
            have.physique,
            tags.get("Strength").unwrap_or("Physique"),
            pct[0],
        ),
        (
            r.cunning as f32,
            have.cunning,
            tags.get("Dexterity").unwrap_or("Cunning"),
            pct[1],
        ),
        (
            r.spirit as f32,
            have.spirit,
            tags.get("Intelligence").unwrap_or("Spirit"),
            pct[2],
        ),
    ] {
        if need <= 0.0 {
            continue;
        }
        let reduced = super::req_reduction::reduced(need, pct);
        let (key, met) = if have >= need {
            ("MeetsRequirement", true)
        } else if have >= reduced {
            ("MeetsRequirementWithReduction", true)
        } else if pct > 0.0 {
            ("FailsRequirementWithReduction", false)
        } else {
            ("FailsRequirement", false)
        };
        let f = tags.get(key).unwrap_or("Required {%s0}: {%.0f1}");
        let args = [
            Arg::Str(label),
            Arg::Num(if key == "MeetsRequirement" || key == "FailsRequirement" {
                need
            } else {
                reduced
            }),
            Arg::Num(need),
            Arg::Num(pct),
        ];
        out.push(Line {
            text: format(f, &args),
            kind: LineKind::Requirement { met },
            class: tc::REQUIREMENTS,
            icon: None,
        });
    }
    out
}

/// Store order of the text lists (damage, conversion, character, defense,
/// retaliation, skill, requirement reduction).
pub fn store_rank(a: &Attr) -> u8 {
    let reduction = a.store == StoreKind::Char
        && a.values
            .first()
            .is_some_and(|(k, _)| k.ends_with("ReqReduction"));
    match a.store {
        StoreKind::Damage => 0,
        StoreKind::Conversion => 1,
        StoreKind::Char if reduction => 6,
        StoreKind::Char => 2,
        StoreKind::Defense => 3,
        StoreKind::Retaliation => 4,
        StoreKind::Skill => 5,
    }
}

/// The attribute lines of an item (base lines class 18, the rest class 19)
/// in the game's order, for the attributes `keep` accepts.
pub fn stat_lines(tags: &TagTable, item: &ResolvedItem, keep: impl Fn(&Attr) -> bool) -> Vec<Line> {
    stat_lines_with(tags, item, keep, true)
}

/// [`stat_lines`] with or without the `^X` colour codes.
pub fn stat_lines_with(
    tags: &TagTable,
    item: &ResolvedItem,
    keep: impl Fn(&Attr) -> bool,
    colors: bool,
) -> Vec<Line> {
    stat_lines_ranged(tags, item, keep, colors, None)
}

/// [`stat_lines_with`]; with `bounds` (the details view) each rolled
/// attribute of the item's own stores gets its [`range_numbers`] appended.
/// A global-chance damage attribute gets none (the range loop walks
/// the store's non-global list; the global ones print in `CreateGlobalText`).
pub fn stat_lines_ranged(
    tags: &TagTable,
    item: &ResolvedItem,
    keep: impl Fn(&Attr) -> bool,
    colors: bool,
    bounds: Option<&super::resolve::AttrBounds>,
) -> Vec<Line> {
    stat_lines_ranged_speed(tags, item, keep, colors, bounds, None)
}

/// [`stat_lines_ranged`] with the weapon base attack speed line's player
/// context: `speed` = (the player's base attack speed, the weapon
/// animation's frame rate / length), [`attack_speed_line`]; `None` leaves
/// the line out (a read-only text without a player).
pub fn stat_lines_ranged_speed(
    tags: &TagTable,
    item: &ResolvedItem,
    keep: impl Fn(&Attr) -> bool,
    colors: bool,
    bounds: Option<&super::resolve::AttrBounds>,
    speed: Option<(f32, f32)>,
) -> Vec<Line> {
    let mut out = Vec::new();
    // (section, store, base-first) per attribute; `None` = not shown.
    let weapon_armor = item.class.starts_with("WeaponArmor");
    let weapon = !weapon_armor && item.class.starts_with("Weapon");
    let rank = |a: &Attr| -> Option<(u8, u8, u8)> {
        let dmg_base = a.class == "DamageAttribute_BasePhysical"
            || a.class.starts_with("DamageAttributeAbsBase");
        let def_base = a.class == "DefenseAttribute_BaseProtectionAbsorption";
        let speed_base = a.class == "CharAttributeVal_BaseAttackSpeed";
        let store = store_rank(a);
        Some(match () {
            _ if weapon && dmg_base => (0, 0, 0),
            _ if weapon && speed_base => (0, 1, 0),
            _ if weapon_armor && dmg_base => (0, 0, 0),
            _ if weapon_armor && def_base => (0, 1, 0),
            _ if weapon_armor && speed_base => return None,
            _ if !weapon && !weapon_armor && def_base => (0, 0, 0),
            _ => (1, store, u8::from(!(dmg_base || def_base || speed_base))),
        })
    };
    let mut attrs: Vec<(usize, &Attr, (u8, u8, u8))> = item
        .attrs
        .iter()
        .enumerate()
        .filter(|(_, a)| keep(a))
        .filter_map(|(i, a)| rank(a).map(|r| (i, a, r)))
        .collect();
    attrs.sort_by_key(|x| x.2);
    for (i, a, r) in attrs {
        let Some(mut text) = attr_text(tags, a, speed, colors) else {
            continue;
        };
        // The slot's `_Min` / `_Max` attributes: the bound sets have the
        // rolled set's creators and order, so entry `i` is the same slot.
        if let Some(b) = bounds
            && a.global.is_none()
            && let (Some(lo), Some(hi)) = (b.min.get(i), b.max.get(i))
            && lo.class == a.class
            && hi.class == a.class
            && let Some(r) = range_numbers(tags, lo, hi, colors)
        {
            text.push_str(&r);
        }
        let (kind, class) = if r.0 == 0 {
            (LineKind::Base, tc::BASE_STATS)
        } else {
            (LineKind::Bonus, tc::BONUSES)
        };
        for l in text.lines() {
            out.push(Line {
                text: l.to_string(),
                kind,
                class,
                icon: None,
            });
        }
    }
    out
}

/// Name, every attribute and the requirements of an item (the compact form;
/// [`super::tooltip_item::item_tooltip`] builds the whole game tooltip).
pub fn lines(tags: &TagTable, item: &ResolvedItem, name: &str, have: Have) -> Vec<Line> {
    let mut out = vec![Line {
        text: name.to_string(),
        kind: LineKind::Name,
        class: tc::DEFAULT,
        icon: None,
    }];
    out.extend(stat_lines_with(tags, item, |_| true, false));
    out.extend(requirement_lines(tags, &item.requirements, have));
    out
}
