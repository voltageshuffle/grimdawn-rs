//! Item styles: user rules that recolour item names and add a short type tag (a port addition in the
//! spirit of the "Grim Raynbow" / "Rainbow Filter" mods; the real game has no such option).
//!
//! A [`StyleRules`] is an ordered list of [`Rule`]s. Each rule has conditions (all must hold; an empty
//! condition is "any") and a [`Style`]. **The first rule that matches an item wins**: its style is the
//! item's, later rules are not looked at ([`StyleRules::find`]); no matching rule leaves the stock look.
//!
//! Conditions, read from what the item already exposes (the loot filter's classification,
//! [`super::loot_filter`], and [`super::illusion::item_type`]'s class groups):
//! - rarity ([`Rarity`]): the item's classification (`ResolvedItem::classification`, the loot filter's
//!   quality classes 0–4); only equipment carries one, so a rarity condition never matches a component,
//!   potion, ...;
//! - type ([`Kind`]): the class family ([`kind_of`]);
//! - traits ([`Trait`], each any / yes / no): part of a set, monster infrequent (the base record alone is
//!   Rare), double rare (Rare prefix and suffix), ascended (an ascendant affix): the loot filter's options
//!   6, 3, 38, 39;
//! - item level range (`itemLevel`, inclusive; either end open);
//! - a case-insensitive substring of the shown name (with the stack count, without markup).
//!
//! Style: the name colour (`None`: the stock rarity colour), an optional tag text put before or after the
//! name, and "hide label" (the loot filter's side: `gd_game::loot_filter`).
//!
//! File format ([`StyleRules::parse`] / [`StyleRules::write`]): UTF-8 text, one rule per line, `#` starts a
//! comment line, blank lines are skipped. A rule is whitespace-separated `key=value` tokens (a value with
//! spaces in double quotes, `\"` and `\\` escaped) plus the bare word `hide`:
//! `rarity=common,magic,rare,epic,legendary`, `type=weapon,offhand,armor,accessory,component,relic,
//! blueprint,augment,consumable,material,quest,lore,other`, `set=yes|no`, `mi=yes|no`,
//! `doublerare=yes|no`, `ascended=yes|no`, `level=MIN-MAX` (`MIN-`, `-MAX` or one number),
//! `name="text"`, `color=#RRGGBB`, `prefix="text"` or `suffix="text"` (trimmed; one space is put between tag
//! and name), `hide`. A line with an unknown key or a bad value is skipped with a warning; the others still load.
//! Saving keeps the user's comment lines and turns a skipped line into a `# skipped: <line>` comment, all gathered
//! at the top of the file (blank lines are not kept); a file without comments gets a short header.
use super::illusion::item_type;
use super::loot_filter::{is_accessory, is_armor};
use super::resolve::{Classification, ResolvedItem};
use gd_formats::text::TagTable;
use std::fmt::Write as _;

/// The rarity condition's classes (the loot filter's quality classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rarity {
    Common,
    Magic,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub const ALL: [Rarity; 5] = [
        Rarity::Common,
        Rarity::Magic,
        Rarity::Rare,
        Rarity::Epic,
        Rarity::Legendary,
    ];

    /// The file's word.
    pub fn key(self) -> &'static str {
        ["common", "magic", "rare", "epic", "legendary"][self as usize]
    }

    fn of(c: Classification) -> Option<Rarity> {
        Some(match c {
            Classification::Common => Rarity::Common,
            Classification::Magical => Rarity::Magic,
            Classification::Rare => Rarity::Rare,
            Classification::Epic => Rarity::Epic,
            Classification::Legendary => Rarity::Legendary,
            _ => return None,
        })
    }
}

/// The type condition's class families ([`kind_of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Melee, ranged and caster weapons (item type 9).
    Weapon,
    /// Shields and caster off-hands (item type 8).
    Offhand,
    /// The loot filter's Armor group ([`is_armor`]).
    Armor,
    /// The loot filter's Accessories group ([`is_accessory`]).
    Accessory,
    /// `ItemRelic` / `ItemCharm` (the game's components).
    Component,
    /// `ItemArtifact` (the game's relics).
    Relic,
    /// The formula classes (blueprints).
    Blueprint,
    /// `ItemEnchantment` (augments).
    Augment,
    /// `OneShot_*` (potions, elixirs, scrolls), `ItemUsableSkill`, faction writs, the reset and
    /// difficulty-unlock items.
    Consumable,
    /// Any other `craftingMaterial` record (stock: the `QuestItem`s under `crafting/materials`).
    Material,
    /// Any other Quest classification.
    Quest,
    /// `ItemNote`.
    Lore,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Weapon,
        Kind::Offhand,
        Kind::Armor,
        Kind::Accessory,
        Kind::Component,
        Kind::Relic,
        Kind::Blueprint,
        Kind::Augment,
        Kind::Consumable,
        Kind::Material,
        Kind::Quest,
        Kind::Lore,
        Kind::Other,
    ];

    /// The file's word.
    pub fn key(self) -> &'static str {
        [
            "weapon",
            "offhand",
            "armor",
            "accessory",
            "component",
            "relic",
            "blueprint",
            "augment",
            "consumable",
            "material",
            "quest",
            "lore",
            "other",
        ][self as usize]
    }
}

/// The class family of an item (`crafting_material`: its base record's `craftingMaterial`; components carry it too,
/// so the class families come first).
pub fn kind_of(r: &ResolvedItem, crafting_material: bool) -> Kind {
    match item_type(&r.class) {
        9 => Kind::Weapon,
        8 => Kind::Offhand,
        t if is_armor(t) => Kind::Armor,
        t if is_accessory(t) => Kind::Accessory,
        16 => Kind::Component,
        11 => Kind::Relic,
        12 => Kind::Blueprint,
        21 => Kind::Augment,
        17 | 23..=26 => Kind::Consumable,
        20 => Kind::Lore,
        _ if crafting_material => Kind::Material,
        _ if r.class == "ItemUsableSkill" || r.classification == Classification::Potion => {
            Kind::Consumable
        }
        _ if r.classification == Classification::Quest => Kind::Quest,
        _ => Kind::Other,
    }
}

/// The tri-state trait conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trait {
    Set,
    MonsterInfrequent,
    DoubleRare,
    Ascended,
}

impl Trait {
    pub const ALL: [Trait; 4] = [
        Trait::Set,
        Trait::MonsterInfrequent,
        Trait::DoubleRare,
        Trait::Ascended,
    ];

    /// The file's word.
    pub fn key(self) -> &'static str {
        ["set", "mi", "doublerare", "ascended"][self as usize]
    }

    fn holds(self, r: &ResolvedItem) -> bool {
        match self {
            Trait::Set => r.item_set.is_some(),
            Trait::MonsterInfrequent => r.base_classification == Classification::Rare,
            Trait::DoubleRare => {
                r.prefix_classification == Some(Classification::Rare)
                    && r.suffix_classification == Some(Classification::Rare)
            }
            Trait::Ascended => !r.item.ascendant_name.is_empty(),
        }
    }
}

/// Where the tag text goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TagPos {
    #[default]
    Prefix,
    Suffix,
}

/// What a matching rule does to the item's name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    /// The name colour (sRGB); `None` keeps the stock rarity colour.
    pub color: Option<[u8; 3]>,
    /// Text put before or after the name with a space between ("" none; trimmed).
    pub tag: String,
    pub tag_pos: TagPos,
    /// The ground label is hidden (`gd_game::loot_filter`).
    pub hide: bool,
}

impl Style {
    /// The colour as 0..1 sRGB floats.
    pub fn rgb(&self) -> Option<[f32; 3]> {
        self.color.map(|c| c.map(|v| v as f32 / 255.0))
    }

    /// Adds the tag to `name` in place, one space between them.
    pub fn apply_tag(&self, name: &mut String) {
        if self.tag.is_empty() {
            return;
        }
        match self.tag_pos {
            TagPos::Prefix => {
                name.insert(0, ' ');
                name.insert_str(0, &self.tag);
            }
            TagPos::Suffix => {
                name.push(' ');
                name.push_str(&self.tag);
            }
        }
    }
}

/// One rule: conditions and the style they give (module doc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    /// Bit `Rarity as u8` per allowed rarity; 0 = any.
    pub rarity: u8,
    /// Bit `Kind as u16` per allowed family; 0 = any.
    pub kinds: u16,
    /// Per [`Trait`]: `None` any, `Some(true)` must hold, `Some(false)` must not.
    pub traits: [Option<bool>; 4],
    /// Item level bounds, inclusive.
    pub level_min: Option<u32>,
    pub level_max: Option<u32>,
    /// Case-insensitive substring of the shown name ("" any).
    pub name: String,
    pub style: Style,
}

impl Rule {
    pub fn has_rarity(&self, r: Rarity) -> bool {
        self.rarity & (1 << r as u8) != 0
    }

    pub fn toggle_rarity(&mut self, r: Rarity) {
        self.rarity ^= 1 << r as u8;
    }

    pub fn has_kind(&self, k: Kind) -> bool {
        self.kinds & (1 << k as u16) != 0
    }

    pub fn toggle_kind(&mut self, k: Kind) {
        self.kinds ^= 1 << k as u16;
    }

    /// The conditions as the file writes them ("" for a rule that matches everything).
    pub fn conditions(&self) -> String {
        let mut out = String::new();
        self.write_conditions(&mut out);
        out.trim_start().to_owned()
    }

    fn write_conditions(&self, out: &mut String) {
        let list = |out: &mut String, key: &str, words: &mut dyn Iterator<Item = &str>| {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            for (i, w) in words.enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(w);
            }
        };
        if self.rarity != 0 {
            list(
                out,
                "rarity",
                &mut Rarity::ALL
                    .iter()
                    .filter(|r| self.has_rarity(**r))
                    .map(|r| r.key()),
            );
        }
        if self.kinds != 0 {
            list(
                out,
                "type",
                &mut Kind::ALL
                    .iter()
                    .filter(|k| self.has_kind(**k))
                    .map(|k| k.key()),
            );
        }
        for t in Trait::ALL {
            if let Some(v) = self.traits[t as usize] {
                out.push(' ');
                out.push_str(t.key());
                out.push_str(if v { "=yes" } else { "=no" });
            }
        }
        match (self.level_min, self.level_max) {
            (None, None) => {}
            (a, b) => {
                let n = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
                write!(out, " level={}-{}", n(a), n(b)).expect("a String takes any text");
            }
        }
        if !self.name.is_empty() {
            out.push_str(" name=");
            quote(out, &self.name);
        }
    }

    fn matches(&self, item: &StyledItem, kind: &mut Option<Kind>) -> bool {
        let r = item.resolved;
        if self.rarity != 0 && !Rarity::of(r.classification).is_some_and(|x| self.has_rarity(x)) {
            return false;
        }
        if self.kinds != 0 {
            let k = *kind.get_or_insert_with(|| kind_of(r, item.crafting_material));
            if !self.has_kind(k) {
                return false;
            }
        }
        if Trait::ALL
            .iter()
            .any(|t| self.traits[*t as usize].is_some_and(|want| t.holds(r) != want))
        {
            return false;
        }
        if self.level_min.is_some_and(|m| r.item_level < m)
            || self.level_max.is_some_and(|m| r.item_level > m)
        {
            return false;
        }
        if !self.name.is_empty() {
            return match item.name {
                Name::Shown(n) => contains_ci(n, &self.name),
                // Only a rule that got this far with a name condition builds the name.
                Name::Tags(tags) => contains_ci(&super::naming::item_name(tags, r), &self.name),
            };
        }
        true
    }
}

/// The item's shown name for the name condition: the text already built (label, tooltip), or the tag
/// table to build it from when a rule asks (the filter's hide test).
#[derive(Clone, Copy)]
pub enum Name<'a> {
    Shown(&'a str),
    Tags(&'a TagTable),
}

/// What the rules read of an item.
#[derive(Clone, Copy)]
pub struct StyledItem<'a> {
    pub resolved: &'a ResolvedItem,
    /// The base record's `craftingMaterial`.
    pub crafting_material: bool,
    pub name: Name<'a>,
}

impl<'a> StyledItem<'a> {
    /// `base`: the item's base record (its `craftingMaterial`).
    pub fn new(
        resolved: &'a ResolvedItem,
        base: Option<&gd_formats::arz::Record>,
        name: Name<'a>,
    ) -> Self {
        Self {
            resolved,
            crafting_material: base.is_some_and(|b| b.bool("craftingMaterial")),
            name,
        }
    }
}

/// Whether `hay` contains `needle`, case-insensitively, without allocating.
fn contains_ci(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.char_indices().any(|(i, _)| {
        let mut h = hay[i..].chars().flat_map(char::to_lowercase);
        needle
            .chars()
            .flat_map(char::to_lowercase)
            .all(|n| h.next() == Some(n))
    })
}

/// The ordered rules (module doc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleRules {
    pub rules: Vec<Rule>,
    /// The file's comment lines and its skipped lines (as `# skipped: <line>`), written back first (module doc).
    pub comments: Vec<String>,
}

impl StyleRules {
    /// The first rule matching `item` (module doc: first match wins).
    pub fn find(&self, item: &StyledItem) -> Option<&Style> {
        let mut kind = None;
        self.rules
            .iter()
            .find(|r| r.matches(item, &mut kind))
            .map(|r| &r.style)
    }

    /// Whether any rule hides labels (the filter skips the rules otherwise).
    pub fn any_hide(&self) -> bool {
        self.rules.iter().any(|r| r.style.hide)
    }

    /// Parses the file (module doc); the skipped lines' warnings come back with their line numbers.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut rules = Vec::new();
        let mut comments = Vec::new();
        let mut warnings = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('#') {
                comments.push(line.to_owned());
                continue;
            }
            match parse_rule(line) {
                Ok(r) => rules.push(r),
                Err(e) => {
                    warnings.push(format!("line {}: {e}; skipped", n + 1));
                    comments.push(format!("# skipped: {line}"));
                }
            }
        }
        (Self { rules, comments }, warnings)
    }

    /// The file's text (module doc): the kept comment lines (a short header for a file without any), then the rules.
    pub fn write(&self) -> String {
        let mut out = String::new();
        if self.comments.is_empty() {
            out.push_str(
                "# grimdawn-rs item styles: one rule per line, the first rule that matches an item wins.\n\
                 # Conditions: rarity= type= set= mi= doublerare= ascended= level=MIN-MAX name=\"...\"\n\
                 # Style: color=#RRGGBB prefix=\"...\" | suffix=\"...\" hide\n",
            );
        }
        for c in &self.comments {
            out.push_str(c);
            out.push('\n');
        }
        for r in &self.rules {
            let mut line = String::new();
            r.write_conditions(&mut line);
            if let Some([a, b, c]) = r.style.color {
                write!(line, " color=#{a:02x}{b:02x}{c:02x}").expect("a String takes any text");
            }
            if !r.style.tag.is_empty() {
                line.push_str(match r.style.tag_pos {
                    TagPos::Prefix => " prefix=",
                    TagPos::Suffix => " suffix=",
                });
                quote(&mut line, &r.style.tag);
            }
            if r.style.hide {
                line.push_str(" hide");
            }
            out.push_str(line.trim_start());
            out.push('\n');
        }
        out
    }

    /// The built-in Raynbow-like preset: tagged, coloured non-gear by family, highlighted set /
    /// monster-infrequent / double-rare / ascended gear, dimmed common gear.
    pub fn preset() -> Self {
        Self::parse(PRESET).0
    }
}

const PRESET: &str = r#"
type=material color=#ff9933 prefix="[Mat]"
type=component color=#ffd700 prefix="[Comp]"
type=blueprint color=#3fc8ff prefix="[BP]"
type=relic color=#ff6a3d prefix="[Relic]"
type=augment color=#7dffc4 prefix="[Aug]"
type=consumable color=#c9a6ff prefix="[Use]"
type=quest color=#ff66cc prefix="[Quest]"
type=lore color=#d9bb8c
ascended=yes color=#ff4d4d suffix="[Asc]"
rarity=epic,legendary set=yes color=#33ff99 suffix="[Set]"
doublerare=yes color=#99ff33 suffix="[DR]"
rarity=rare mi=yes color=#66ff66 suffix="[MI]"
rarity=common type=weapon,offhand,armor,accessory color=#8c8c8c
"#;

/// The palette the loot filter window offers (`color=` presets).
pub const PALETTE: [[u8; 3]; 16] = [
    [0xff, 0xff, 0xff],
    [0x8c, 0x8c, 0x8c],
    [0xff, 0xf0, 0x66],
    [0xff, 0xd7, 0x00],
    [0xff, 0x99, 0x33],
    [0xff, 0x4d, 0x4d],
    [0xff, 0x66, 0xcc],
    [0xcc, 0x33, 0xff],
    [0x99, 0x66, 0xff],
    [0x33, 0x66, 0xff],
    [0x3f, 0xc8, 0xff],
    [0x00, 0xff, 0xff],
    [0x7d, 0xff, 0xc4],
    [0x33, 0xff, 0x99],
    [0x99, 0xff, 0x33],
    [0xd9, 0xbb, 0x8c],
];

/// `#RRGGBB` (or `RRGGBB`) to a colour.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// `MIN-MAX`, `MIN-`, `-MAX` or `N`.
pub fn parse_level(s: &str) -> Option<(Option<u32>, Option<u32>)> {
    let n = |t: &str| -> Option<Option<u32>> {
        let t = t.trim();
        if t.is_empty() {
            Some(None)
        } else {
            t.parse().ok().map(Some)
        }
    };
    match s.split_once('-') {
        Some((a, b)) => Some((n(a)?, n(b)?)),
        None => {
            let v = n(s)?;
            v.map(|v| (Some(v), Some(v)))
        }
    }
}

fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

/// Splits a line into tokens (quoted values unescaped).
fn tokens(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut it = line.chars().peekable();
    loop {
        while it.next_if(|c| c.is_whitespace()).is_some() {}
        if it.peek().is_none() {
            return Ok(out);
        }
        let mut t = String::new();
        while let Some(&c) = it.peek() {
            if c.is_whitespace() {
                break;
            }
            it.next();
            if c != '"' {
                t.push(c);
                continue;
            }
            loop {
                match it.next() {
                    Some('"') => break,
                    Some('\\') => t.push(it.next().ok_or("unfinished escape")?),
                    Some(c) => t.push(c),
                    None => return Err("unclosed quote".into()),
                }
            }
        }
        out.push(t);
    }
}

fn parse_rule(line: &str) -> Result<Rule, String> {
    let mut r = Rule::default();
    for tok in tokens(line)? {
        if tok == "hide" {
            r.style.hide = true;
            continue;
        }
        let (k, v) = tok
            .split_once('=')
            .ok_or_else(|| format!("`{tok}` is not key=value"))?;
        let bad = || format!("bad value `{v}` for `{k}`");
        match k {
            "rarity" => {
                for w in v.split(',') {
                    let x = Rarity::ALL
                        .into_iter()
                        .find(|x| x.key().eq_ignore_ascii_case(w.trim()))
                        .ok_or_else(bad)?;
                    r.rarity |= 1 << x as u8;
                }
            }
            "type" => {
                for w in v.split(',') {
                    let x = Kind::ALL
                        .into_iter()
                        .find(|x| x.key().eq_ignore_ascii_case(w.trim()))
                        .ok_or_else(bad)?;
                    r.kinds |= 1 << x as u16;
                }
            }
            "level" => (r.level_min, r.level_max) = parse_level(v).ok_or_else(bad)?,
            "name" => r.name = v.to_owned(),
            "color" | "colour" => r.style.color = Some(parse_color(v).ok_or_else(bad)?),
            "prefix" | "suffix" => {
                r.style.tag = v.trim().to_owned();
                r.style.tag_pos = if k == "prefix" {
                    TagPos::Prefix
                } else {
                    TagPos::Suffix
                };
            }
            _ => {
                let t = Trait::ALL
                    .into_iter()
                    .find(|t| t.key() == k)
                    .ok_or_else(|| format!("unknown key `{k}`"))?;
                r.traits[t as usize] = Some(match v {
                    "yes" | "true" | "1" => true,
                    "no" | "false" | "0" => false,
                    _ => return Err(bad()),
                });
            }
        }
    }
    Ok(r)
}
