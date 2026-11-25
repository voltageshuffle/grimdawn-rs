//! The buff bar: status icons above the hotbar (`hudPlayerStatusIcons`,
//! `records/ui/hud/orbhud/hud_playerstatusicons.dbr`, the classic HUD's `hud_playerstatusicons.dbr`; Grim Dawn.exe:
//! loader `PlayerStatusIcons::Load`, update `PlayerStatusIcons::Update`, add `PlayerStatusIcons::AddIcon`, refresh
//! `PlayerStatusIcons::RefreshIcon`, icon tick `PlayerStatusIcons::TickIcons`, draw `PlayerStatusIcons::Draw` →
//! `PlayerStatusIcons::DrawArea`, sweep `PlayerStatusIcons::DrawSweep`, hit test `PlayerStatusIcons::HitTest`).
//!
//! Which skills: `gd_game::buff_bar::PlayerIcons` (area 1, or area 2 for
//! debuff buffs) and the duration-damage / debuff / crowd-control types (area
//! 2, one icon per type, `ui/hud/hud_dur…icon_….tex` by type, `PlayerStatusIcons::AddDotIcon`).
//! The bar keeps its icons ([`BuffBar`]): a newly listed skill is appended
//! (so the order is the order things started), a listed one is refreshed,
//! one no longer listed goes — except a passive, which after up to 5 frames
//! waiting for its cooldown to start shows that cooldown until it ends.
//!
//! An icon (`iconWidth`×`iconHeight`) shows:
//! - a timed buff: the elapsed part (total − left) / total darkened by a
//!   clockwise sweep from 12 o'clock in `durationColor` (default 0,0,0,153),
//!   following the square's edge (by perimeter length) or, for a round icon
//!   (`roundBitmap` of it / its parent, or a passive), an ellipse fan in π/16
//!   steps; in area 1 always, in area 2 only with a total; under 3 s left
//!   the whole seconds + 1 in red (1,0,0,1, the record's `buffDurationText`
//!   is overwritten);
//! - charges (pets, Fervor-type pool stacks): the count in `chargeText` (white) and the sweep of (max − count) / max
//!   (pet limit / charge level; `PlayerStatusIcons::RefreshIcon` type 1, `PlayerStatusIcons::DrawArea`); Cadence (type
//!   2) shows neither;
//! - toggles and auras' buffs: nothing;
//! - a passive's cooldown: the elapsed part in `cooldownColor` (record r 200,
//!   default 0,0,153).
//!
//! Text: `cooldownFontStyle` at `cooldownTextLocationX/Y` (× UI scale) in
//! the icon. Layout from the area's corner (`hud::portrait_origin` +
//! `activeHudArea{,2}X/Y`): each icon adds its width + `iconHorizSpace` to the
//! row; left-to-right areas step right by it, the others step left by the
//! next icon's width + space; once the row's sum reaches
//! `activeHudAreaSize{,2}X` the next icon starts a new row at the area's x,
//! `iconHeight` + `iconVertSpace` up (`…GrowUp`) or down. The stock record
//! fits 6 icons per row, rows growing up; area 2 starts at x 669 and grows
//! leftwards.
//!
//! Anchor (HUD loader `UIPlayerHud::Load`): the widget is offset by the
//! `hudPortraitAndStatsSummary` x and the `hudToolbar` top less the
//! portrait's height, as `hud::portrait_origin`.
//!
//! PORT: a timed buff's total is the most time left seen since it appeared
//! (the engine asks the skill, `TrackableTotalTime`), likewise a cooldown's;
//! the sweep is a rasterised bitmap in 64 steps
//! (the engine draws a triangle fan); a changed bar (icons, their shape or text colour) is a new node tree that
//! replaces the old one once drawn (`retire_roots`), while the sweeps and texts change in place; the hover
//! tooltip is the skill's tooltip (as on the hotbar), placed by the rollover window's mode 4 (centred below the
//! icon, clamped to the screen; `tip::place_below`). Both tooltips hide while a window covers the bar / hotbar:
//! the pause menu, the map / journal (`skills::Above`) and a registered window button / blocker in front of the
//! rect (`widgets::frontmost`, as `rollover::hud_rollover`). The icons live in their own node tree (not
//! [`super::UiRoot`]); their values are [`super::HudVitals::status`].
use super::{HudValues, Ui};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use gd_formats::collections::HashMap;
use gd_formats::ui::Rect as URect;
use gd_game::buff_bar::{Track, Tracked};
use gd_game_core::data::GameData;

/// The master table's `hudPlayerStatusIcons` record: `orbhud/hud_playerstatusicons.dbr` or, with `standardHUD`,
/// `hud_playerstatusicons.dbr` (`hud.rs` module doc).
fn record(ui: &Ui) -> Option<gd_formats::arz::Record> {
    let rec = ui
        .record(ui.hud_master())?
        .str("hudPlayerStatusIcons")?
        .to_string();
    ui.record(&rec)
}

/// An icon's size without `iconWidth` / `iconHeight` (the classic record has none): `PlayerStatusIcons::AddIcon`
/// `SetSize(0, 0)`, and the bitmap's rect getter `UIBitmap::GetRect` then gives the texture's own size (stored by
/// `SetTexture` `UIBitmap::SetTexture`); before a texture loads the rect is 32 × 32 (`UIBitmap::defaultRect`). Each
/// icon steps the row by its own width (`PlayerStatusIcons::DrawArea`); that size is not UI-scaled (the record's spaces
/// are). Stock skill icons are mostly 32 × 32 (some 34, 40, 52).
const UNLOADED_ICON: f32 = 32.0;

/// Sweep steps of the rasterised overlay.
const SWEEP_STEPS: u8 = 64;

/// A duration-damage / debuff / crowd-control icon of the bar: its bitmap, seconds left, type and magnitude (the
/// tooltip's numbers).
#[derive(Clone, Debug)]
pub struct DotIcon {
    pub icon: &'static str,
    pub left: f32,
    pub ty: gd_sim::attributes::CombatType,
    pub magnitude: i32,
}

/// One icon of the target info widget's debuff strip (`monsterBarsDebuffs`): the bitmap and the red countdown
/// (whole seconds + 1 while under 3 s of a timed debuff left, `PlayerStatusIcons::RefreshIcon` type 0).
#[derive(Clone, Debug, PartialEq)]
pub struct TargetDebuff {
    pub icon: String,
    pub text: Option<String>,
}

/// One drawn icon (built by [`BuffBar::update`]).
#[derive(Clone, Debug, PartialEq)]
pub struct StatusIcon {
    /// Bitmap path.
    pub icon: String,
    /// The skill whose tooltip it shows.
    pub tip: Option<String>,
    /// Area 2 (debuffs) rather than area 1 (buffs).
    pub debuff: bool,
    pub round: bool,
    /// Text drawn on it: (text, charge colour rather than the red duration).
    pub text: Option<(String, bool)>,
    /// Elapsed part in [`SWEEP_STEPS`] steps (0: none) and whether it is a
    /// cooldown (`cooldownColor`) rather than a duration.
    pub sweep: (u8, bool),
    /// The combo charge icon (kind 2): multiplier (%) and ms left; its rollover is
    /// the engine's own text (`PlayerStatusIcons::GetRolloverText`), not a skill's.
    pub combo: Option<(i64, i32)>,
    /// A duration-damage icon (kind 1): type, magnitude and ms left (the tooltip, [`dot_lines`]).
    pub dot: Option<(gd_sim::attributes::CombatType, i32, i32)>,
    /// A potion's absorb (type 4): the shield left (`Skill.absorbLeft`), for its tooltip line
    /// (`gd_sim::skill_tooltip::potion_tracker_lines`).
    pub absorb: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// Listed (state 0).
    Shown,
    /// A passive's cooldown after its buff (state 1).
    Cooldown,
}

#[derive(Clone, Debug)]
struct Entry {
    key: String,
    icon: String,
    tip: Option<String>,
    debuff: bool,
    round: bool,
    passive: bool,
    cooldown: String,
    phase: Phase,
    seen: bool,
    /// Frames left to wait for a passive's cooldown (`StatusIcon.cooldownGrace`, 5).
    grace: u8,
    /// Seconds left / total (0: none).
    left: f32,
    total: f32,
    text: Option<(String, bool)>,
    /// The combo charge icon: its multiplier (%).
    combo: Option<i64>,
    /// A duration-damage icon: its type and magnitude.
    dot: Option<(gd_sim::attributes::CombatType, i32)>,
    /// A potion's absorb left.
    absorb: Option<f32>,
}

/// The bar's icons in display order (the widget's two icon lists).
#[derive(Default)]
pub struct BuffBar(Vec<Entry>);

impl BuffBar {
    /// One frame: `listed` = the player's tracked skills, `dots` = active
    /// duration-damage / debuff types (bitmap, seconds left), `cooldown` =
    /// a skill's cooldown left; the drawn icons are written into `out` in place (an icon's strings keep their
    /// buffers).
    pub fn update(
        &mut self,
        listed: &[Tracked],
        dots: &[DotIcon],
        cooldown: impl Fn(&str) -> f32,
        out: &mut Vec<StatusIcon>,
    ) {
        for e in &mut self.0 {
            e.seen = false;
        }
        for t in listed {
            let i = match self
                .0
                .iter()
                .position(|e| e.key == t.key && e.debuff == t.debuff)
            {
                Some(i) => i,
                None => {
                    self.0.push(Entry {
                        key: t.key.clone(),
                        icon: t.icon.clone(),
                        tip: Some(t.tip.clone()),
                        debuff: t.debuff,
                        round: t.round,
                        passive: t.passive,
                        cooldown: t.cooldown.clone(),
                        phase: Phase::Shown,
                        seen: false,
                        grace: 5,
                        left: 0.0,
                        total: 0.0,
                        text: None,
                        combo: None,
                        dot: None,
                        absorb: None,
                    });
                    self.0.len() - 1
                }
            };
            let e = &mut self.0[i];
            e.seen = true;
            e.phase = Phase::Shown;
            e.grace = 5;
            match t.track {
                Track::Duration(left) => {
                    if left > e.left {
                        e.total = left;
                    }
                    e.left = left;
                    // `PlayerStatusIcons::RefreshIcon` type 0: under 3000 ms, ms / 1000 + 1.
                    let ms = (left * 1000.0) as i32;
                    if ms < 3000 && e.total > 0.0 {
                        set_count(&mut e.text, ms / 1000 + 1, false);
                    } else {
                        e.text = None;
                    }
                }
                // `PlayerStatusIcons::RefreshIcon` type 1: count = TrackableRemainingTime, total = TrackableTotalTime.
                Track::Charges(n, max) => {
                    (e.left, e.total) = (n as f32, max as f32);
                    if n > 0 {
                        set_count(&mut e.text, n, true);
                    } else {
                        e.text = None;
                    }
                }
                Track::Plain => {
                    (e.left, e.total, e.text) = (0.0, 0.0, None);
                }
                // `PlayerStatusIcons::RefreshIcon` type 4: left / total, no text.
                Track::Gauge { left, total } => {
                    (e.left, e.total, e.text) = (left, total, None);
                    // `CreateUITrackerText` prints `Skill.absorbLeft`, the shield left.
                    e.absorb = Some(left);
                }
                // `PlayerStatusIcons::UpdateComboCharge`: the count, the time left against the most
                // seen (≤ 250 ms: none).
                Track::Combo {
                    count,
                    left,
                    multiplier,
                } => {
                    e.combo = Some(multiplier.round() as i64);
                    if left > e.total {
                        e.total = if left <= 0.25 { 0.0 } else { left };
                    }
                    e.left = left;
                    set_count(&mut e.text, count, true);
                }
            }
        }
        for d in dots {
            let (icon, left) = (&d.icon, &d.left);
            let key_matches = |e: &Entry| e.key.strip_prefix("dot:").is_some_and(|k| k == *icon);
            let i = match self.0.iter().position(key_matches) {
                Some(i) => i,
                None => {
                    let e = Entry {
                        key: format!("dot:{icon}"),
                        icon: (*icon).to_owned(),
                        tip: None,
                        debuff: true,
                        round: false,
                        passive: false,
                        cooldown: String::new(),
                        phase: Phase::Shown,
                        seen: false,
                        grace: 0,
                        left: 0.0,
                        total: 0.0,
                        text: None,
                        combo: None,
                        dot: None,
                        absorb: None,
                    };
                    self.0.push(e);
                    self.0.len() - 1
                }
            };
            let e = &mut self.0[i];
            e.seen = true;
            e.dot = Some((d.ty, d.magnitude));
            // `PlayerStatusIcons::AddDotIcon`: the total is the longest seen (≤ 250 ms: 0).
            if *left > e.total {
                e.total = if *left <= 0.25 { 0.0 } else { *left };
            }
            e.left = *left;
        }
        // `PlayerStatusIcons::TickIcons`: unlisted icons go, a passive's after its cooldown.
        self.0.retain_mut(|e| {
            if e.seen {
                return true;
            }
            match e.phase {
                Phase::Shown => {
                    if !e.passive {
                        return false;
                    }
                    if e.grace > 0 {
                        e.grace -= 1;
                        if cooldown(&e.cooldown) <= 0.0 {
                            return true;
                        }
                    }
                    e.phase = Phase::Cooldown;
                    e.text = None;
                    e.total = 0.0;
                    Self::cooldown_tick(e, &cooldown)
                }
                Phase::Cooldown => Self::cooldown_tick(e, &cooldown),
            }
        });
        out.truncate(self.0.len());
        for (n, e) in self.0.iter().enumerate() {
            let frac = if e.total > 0.0 {
                ((e.total - e.left) / e.total).max(0.0)
            } else {
                0.0
            };
            let step = (frac * SWEEP_STEPS as f32).round().min(SWEEP_STEPS as f32) as u8;
            // `PlayerStatusIcons::DrawArea`: a duration sweep in area 1, or in area 2
            // with a total; a cooldown sweep in the cooldown phase.
            let sweep = match e.phase {
                Phase::Shown if !e.debuff || e.total > 0.0 => (step, false),
                Phase::Cooldown => (step, true),
                _ => (0, false),
            };
            let text = if e.phase == Phase::Shown {
                e.text.as_ref()
            } else {
                None
            };
            let combo = e.combo.map(|m| (m, (e.left * 1000.0) as i32));
            let dot = e.dot.map(|(t, m)| (t, m, (e.left * 1000.0) as i32));
            let Some(o) = out.get_mut(n) else {
                out.push(StatusIcon {
                    icon: e.icon.clone(),
                    tip: e.tip.clone(),
                    debuff: e.debuff,
                    round: e.round,
                    text: text.cloned(),
                    sweep,
                    combo,
                    dot,
                    absorb: e.absorb,
                });
                continue;
            };
            if o.icon != e.icon {
                o.icon.clone_from(&e.icon);
            }
            if o.tip != e.tip {
                o.tip.clone_from(&e.tip);
            }
            if o.text.as_ref() != text {
                o.text = text.cloned();
            }
            (o.debuff, o.round, o.sweep, o.combo, o.dot, o.absorb) =
                (e.debuff, e.round, sweep, combo, dot, e.absorb);
        }
    }

    /// State 1: kept while 0 < cooldown left ≤ its total.
    fn cooldown_tick(e: &mut Entry, cooldown: &impl Fn(&str) -> f32) -> bool {
        let left = cooldown(&e.cooldown);
        if left > e.total {
            e.total = left;
        }
        e.left = left;
        left > 0.0 && left <= e.total
    }
}

/// Writes `n` as an icon's text, reusing its buffer (the counts change every frame: no allocation per frame).
fn set_count(text: &mut Option<(String, bool)>, n: impl std::fmt::Display, charge: bool) {
    use std::fmt::Write;
    let (s, c) = text.get_or_insert_with(Default::default);
    s.clear();
    write!(s, "{n}").unwrap();
    *c = charge;
}

/// The tooltip of a duration-damage / debuff / crowd-control icon (`PlayerStatusIcons::GetRolloverText` kind 1, its
/// type tables): the type's name (class 46); with a magnitude, its description line
/// (class 49: damage every second, reduction % or absolute, chance to miss, damage taken); then the seconds remaining
/// (`tagDotTimeRemainingPrecise` under 1 s). Types without a name tag (the engine's others) have no tooltip.
pub fn dot_lines(
    fonts: &super::text::FontCache,
    ty: gd_sim::attributes::CombatType,
    magnitude: i32,
    ms: i32,
) -> Vec<gd_sim::skill_tooltip::TextLine> {
    use super::floating::format_text;
    use gd_game_core::floating::TextArg;
    use gd_sim::attributes::CombatType as T;
    use gd_sim::skill_tooltip::{TextLine, class};
    const DOT: &str = "tagDurationDamageOverTime";
    const PCT: &str = "tagDotReductionPercent";
    const ABS: &str = "tagDotReductionAbsolute";
    const MISS: &str = "tagDurationFumble";
    let (name, desc): (&str, Option<&str>) = match ty {
        T::Physical => ("tagDotPhysical", Some(DOT)),
        T::Cold => ("tagDotCold", Some(DOT)),
        T::Fire => ("tagDotFire", Some(DOT)),
        T::Acid => ("tagDotPoison", Some(DOT)),
        T::Lightning => ("tagDotLightning", Some(DOT)),
        T::Vitality => ("tagDotVitality", Some(DOT)),
        T::Chaos => ("tagDotChaos", Some(DOT)),
        T::Aether => ("tagDotAether", Some(DOT)),
        T::Bleeding => ("tagDotBleeding", Some(DOT)),
        T::AttackSpeed => ("tagDotAttackSpeed", Some(PCT)),
        T::CastSpeed => ("tagDotCastSpeed", Some(PCT)),
        T::RunSpeed | T::TotalSpeed => ("tagDotRunSpeed", Some(PCT)),
        T::LifeLeech => ("tagDotLifeLeech", Some(DOT)),
        T::ManaLeech => ("tagDotManaLeech", Some(DOT)),
        T::OffensiveAbility => ("tagDotOffensiveAbility", Some(ABS)),
        T::DefensiveAbility => ("tagDotDefensiveAbility", Some(ABS)),
        T::Fumble => ("tagDotFumble", Some(MISS)),
        T::ProjectileFumble => ("tagDotRangedFumble", Some(MISS)),
        T::TotalDamageReductionPercent => ("tagDotTotalDamageReduction", Some(PCT)),
        T::PhysicalDamageReductionPercent | T::OffensiveReduction => {
            ("tagDotPhysicalDamageReduction", Some(PCT))
        }
        T::ElementalDamageReductionPercent => ("tagDotElementalDamageReduction", Some(PCT)),
        T::TotalResistanceReductionPercent => ("tagDotTotalResistReduction", Some(PCT)),
        T::TotalResistanceReductionAbsolute => ("tagDotTotalResistReduction", Some(ABS)),
        T::PhysicalResistanceReductionPercent => ("tagDotPhysicalResistReduction", Some(PCT)),
        T::PhysicalResistanceReductionAbsolute => ("tagDotPhysicalResistReduction", Some(ABS)),
        T::ElementalResistanceReductionPercent => ("tagDotElementalResistReduction", Some(PCT)),
        T::ElementalResistanceReductionAbsolute => ("tagDotElementalResistReduction", Some(ABS)),
        T::Stun | T::Knockdown => ("tagDotStun", None),
        T::Sleep => ("tagDotSleep", None),
        T::Trap | T::Immobilize => ("tagDotTrap", None),
        T::Freeze => ("tagDotFreeze", None),
        T::Petrify => ("tagDotPetrify", None),
        T::Confusion => ("tagDotConfuse", None),
        _ => return Vec::new(),
    };
    let line = |class, text| TextLine {
        class,
        text,
        icon: None,
    };
    let mut out = vec![line(class::SKILL_NAME, format_text(fonts, name, &[]))];
    if magnitude != 0
        && let Some(d) = desc
    {
        out.push(line(
            class::SKILL_STATS_CURRENT,
            format_text(fonts, d, &[TextArg::Num(magnitude as f32)]),
        ));
    }
    let tag = if ms < 1000 {
        "tagDotTimeRemainingPrecise"
    } else {
        "tagDotTimeRemaining"
    };
    out.push(line(
        class::SKILL_STATS_CURRENT,
        format_text(fonts, tag, &[TextArg::Num(ms as f32 * 0.001)]),
    ));
    out
}

/// Texture of a duration-damage / debuff / crowd-control type (`PlayerStatusIcons::AddDotIcon`).
pub fn debuff_icon(ty: gd_sim::attributes::CombatType) -> Option<&'static str> {
    use gd_sim::attributes::CombatType as T;
    Some(match ty {
        T::Physical => "ui/hud/hud_durdamageicon_physical.tex",
        T::Cold => "ui/hud/hud_durdamageicon_cold.tex",
        T::Fire => "ui/hud/hud_durdamageicon_fire.tex",
        T::Acid => "ui/hud/hud_durdamageicon_poison.tex",
        T::Lightning => "ui/hud/hud_durdamageicon_lightning.tex",
        T::Vitality => "ui/hud/hud_durdmgicon_vitalitydecay.tex",
        T::Chaos => "ui/hud/hud_durdamageicon_chaos.tex",
        T::Aether => "ui/hud/hud_durdamageicon_aether.tex",
        T::Bleeding => "ui/hud/hud_durdamageicon_bleeding.tex",
        T::AttackSpeed => "ui/hud/hud_durattrreductionicon_attackspeed.tex",
        T::CastSpeed => "ui/hud/hud_durattrreductionicon_castspeed.tex",
        T::RunSpeed | T::TotalSpeed => "ui/hud/hud_durattrreductionicon_runspeed.tex",
        T::LifeLeech => "ui/hud/hud_durdmgicon_lifeleech.tex",
        T::ManaLeech => "ui/hud/hud_durdmgicon_manaleech.tex",
        T::OffensiveAbility => "ui/hud/hud_durattrreductionicon_offensive.tex",
        T::DefensiveAbility => "ui/hud/hud_durattrreductionicon_defensive.tex",
        T::Fumble => "ui/hud/hud_durattrreductionicon_fumble.tex",
        T::ProjectileFumble => "ui/hud/hud_durattrreductionicon_fumbleranged.tex",
        T::TotalDamageReductionPercent => "ui/hud/hud_durdmgreductionicon_total.tex",
        T::PhysicalDamageReductionPercent | T::OffensiveReduction => {
            "ui/hud/hud_durdmgreductionicon_physical.tex"
        }
        T::ElementalDamageReductionPercent => "ui/hud/hud_durdmgreductionicon_elemental.tex",
        T::TotalResistanceReductionPercent => "ui/hud/hud_durresreductionicon_totalpercent.tex",
        T::TotalResistanceReductionAbsolute => "ui/hud/hud_durresreductionicon_total.tex",
        T::PhysicalResistanceReductionPercent => {
            "ui/hud/hud_durresreductionicon_physicalpercent.tex"
        }
        T::PhysicalResistanceReductionAbsolute => "ui/hud/hud_durresreductionicon_physical.tex",
        T::ElementalResistanceReductionPercent => {
            "ui/hud/hud_durresreductionicon_elementalpercent.tex"
        }
        T::ElementalResistanceReductionAbsolute => "ui/hud/hud_durresreductionicon_elemental.tex",
        T::Stun | T::Knockdown => "ui/hud/hud_durentrapmenticon_stun.tex",
        T::Sleep => "ui/hud/hud_durentrapmenticon_sleep.tex",
        T::Trap | T::Immobilize => "ui/hud/hud_durentrapmenticon_trap.tex",
        T::Freeze => "ui/hud/hud_durentrapmenticon_freeze.tex",
        T::Petrify => "ui/hud/hud_durentrapmenticon_petrify.tex",
        T::Confusion => "ui/hud/hud_durentrapmenticon_confuse.tex",
        _ => return None,
    })
}

#[derive(Component)]
pub struct StatusRoot;

/// The drawn icons' screen rects (`layout`), in the drawn `HudVitals::status` order: [`draw`] writes them when
/// it rebuilds the bar, [`buff_tooltip`] hit-tests them (PORT, a performance choice: no layout per frame).
#[derive(Resource, Default)]
pub struct StatusLayout(Vec<URect>);

/// Screen rects of the icons (module docs: layout), in `icons` order.
fn layout(ui: &mut Ui, icons: &[StatusIcon]) -> Vec<URect> {
    let mut out = vec![URect::new(0.0, 0.0, 0.0, 0.0); icons.len()];
    let (Some(rec), Some(origin)) = (record(ui), super::hud::portrait_origin(ui)) else {
        return out;
    };
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    let (rw, rh) = (f("iconWidth"), f("iconHeight"));
    let sizes: Vec<(f32, f32)> = icons
        .iter()
        .map(|i| {
            let own = ui
                .image(&i.icon)
                .map_or(Vec2::splat(UNLOADED_ICON), |(_, s)| s.as_vec2());
            // The texture's size is in its own pixels (not UI-scaled: the rect getter returns it as stored).
            let w = if rw > 0.0 { ui.sc(rw) } else { own.x };
            let h = if rh > 0.0 { ui.sc(rh) } else { own.y };
            (w, h)
        })
        .collect();
    let (hs, vs) = (ui.sc(f("iconHorizSpace")), ui.sc(f("iconVertSpace")));
    for debuff in [false, true] {
        let p = if debuff { "2" } else { "" };
        let (ax, ay) = (
            origin.x + ui.sc(f(&format!("activeHudArea{p}X"))),
            origin.y + ui.sc(f(&format!("activeHudArea{p}Y"))),
        );
        let aw = ui.sc(f(&format!("activeHudAreaSize{p}X")));
        let ltr = rec.bool(&format!("activeHudArea{p}GrowLeftToRight"));
        let up = rec.bool(&format!("activeHudArea{p}GrowUp"));
        let (mut x, mut y, mut sum) = (ax, ay, 0.0f32);
        let idx: Vec<usize> = (0..icons.len())
            .filter(|&i| icons[i].debuff == debuff)
            .collect();
        for (n, &i) in idx.iter().enumerate() {
            let (iw, ih) = sizes[i];
            out[i] = URect::new(x, y, iw, ih);
            sum += iw + hs;
            if ltr {
                x += iw + hs;
            } else if let Some(&next) = idx.get(n + 1) {
                x -= sizes[next].0 + hs;
            }
            if sum >= aw {
                x = ax;
                sum = 0.0;
                y = if up { y - (ih + vs) } else { y + ih + vs };
            }
        }
    }
    out
}

/// The sweep overlay (`PlayerStatusIcons::DrawSweep`) as a `w`×`h` bitmap: the elapsed part
/// `step / SWEEP_STEPS` clockwise from 12 o'clock, by perimeter length on a
/// square, by angle inside the ellipse on a round icon; 2×2 samples a pixel.
fn sweep_image(w: u32, h: u32, step: u8, round: bool, color: [u8; 4]) -> Image {
    let frac = step as f32 / SWEEP_STEPS as f32;
    let (hw, hh) = (w as f32 * 0.5, h as f32 * 0.5);
    let tau = std::f32::consts::TAU;
    let covered = |px: f32, py: f32| -> bool {
        let (dx, dy) = (px - hw, py - hh);
        if round {
            if (dx / hw).powi(2) + (dy / hh).powi(2) > 1.0 {
                return false;
            }
            // Angle from 12 o'clock, clockwise (y down).
            let a = dx.atan2(-dy).rem_euclid(tau);
            a < frac * tau
        } else {
            // Where the ray from the centre leaves the square, and that
            // point's clockwise perimeter distance from the top centre.
            let t = (hw / dx.abs().max(1e-6)).min(hh / dy.abs().max(1e-6));
            let (ex, ey) = (dx * t, dy * t);
            let (w2, h2) = (hw * 2.0, hh * 2.0);
            let d = if (ey + hh).abs() < 1e-3 && ex >= 0.0 {
                ex
            } else if (ex - hw).abs() < 1e-3 {
                hw + (ey + hh)
            } else if (ey - hh).abs() < 1e-3 {
                hw + h2 + (hw - ex)
            } else if (ex + hw).abs() < 1e-3 {
                hw + h2 + w2 + (hh - ey)
            } else {
                2.0 * (w2 + h2) - hw + (ex + hw)
            };
            d < frac * 2.0 * (w2 + h2)
        }
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let n = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                .iter()
                .filter(|(ox, oy)| covered(x as f32 + ox, y as f32 + oy))
                .count();
            let o = ((y * w + x) * 4) as usize;
            let a = color[3] as f32 * n as f32 / 4.0;
            px[o..o + 4].copy_from_slice(&[color[0], color[1], color[2], a.round() as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Sweep colours (the loader's defaults under the record's fields).
fn sweep_colors(rec: &gd_formats::arz::Record) -> [[u8; 4]; 2] {
    let c = |p: &str, d: [f32; 4]| -> [u8; 4] {
        let k = ["r", "g", "b", "a"];
        std::array::from_fn(|i| {
            rec.f32(&format!("{p}.{}", k[i]))
                .unwrap_or(d[i])
                .clamp(0.0, 255.0) as u8
        })
    };
    [
        c("durationColor", [0.0, 0.0, 0.0, 153.0]),
        c("cooldownColor", [102.0, 0.0, 0.0, 153.0]),
    ]
}

/// An icon's sweep overlay, animated in place by [`draw`]: the icon's index in the bar, its size, shape and the
/// record's two sweep colours.
#[derive(Component)]
pub struct IconSweep {
    index: usize,
    size: (u32, u32),
    round: bool,
    colors: [[u8; 4]; 2],
}

/// An icon's text ([`super::LiveText`]), redrawn in place by [`draw`]: the icon's index in the bar.
#[derive(Component)]
pub struct IconText(usize);

/// What the bar was built for: the window size, the UI scale (`f32` bits) and each icon's bitmap, area, shape
/// and text colour (a charge count is white, a countdown red).
pub struct BarLayout {
    size: UVec2,
    scale: u32,
    icons: Vec<(String, bool, bool, bool)>,
}

impl BarLayout {
    fn key(i: &StatusIcon) -> (&str, bool, bool, bool) {
        (
            &i.icon,
            i.debuff,
            i.round,
            i.text.as_ref().is_some_and(|t| t.1),
        )
    }

    fn same(&self, size: UVec2, scale: u32, icons: &[StatusIcon]) -> bool {
        self.size == size
            && self.scale == scale
            && self.icons.len() == icons.len()
            && self.icons.iter().zip(icons).all(|(a, b)| {
                let k = Self::key(b);
                (a.0.as_str(), a.1, a.2, a.3) == k
            })
    }
}

/// The sweep bitmap of `step` (cached per size, step, shape and colour).
fn sweep_handle(
    sweeps: &mut HashMap<(u32, u32, u8, bool, bool), Handle<Image>>,
    images: &mut Assets<Image>,
    (w, h): (u32, u32),
    (step, cd): (u8, bool),
    round: bool,
    colors: &[[u8; 4]; 2],
) -> Handle<Image> {
    sweeps
        .entry((w, h, step, round, cd))
        .or_insert_with(|| {
            images.add(sweep_image(
                w.max(1),
                h.max(1),
                step,
                round,
                colors[cd as usize],
            ))
        })
        .clone()
}

/// The text of an icon (empty: none).
fn icon_text(i: &StatusIcon) -> &str {
    i.text.as_ref().map_or("", |t| t.0.as_str())
}

/// Builds the bar when its icons (bitmap, area, shape, text colour) or the window change; every other frame
/// swaps the sweep bitmaps and redraws the texts in place.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    vitals: Res<super::HudVitals>,
    roots: Query<(Entity, &super::RootFrame), With<StatusRoot>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    mut last: Local<Option<BarLayout>>,
    mut classic: Local<bool>,
    mut sweeps: Local<HashMap<(u32, u32, u8, bool, bool), Handle<Image>>>,
    (panels, mut built): (Res<super::UiPanels>, ResMut<StatusLayout>),
    mut assets: gd_ui_kit::UiAssets,
    (mut sweep_nodes, mut texts, parents): (
        Query<(&IconSweep, &mut ImageNode, &mut Visibility)>,
        Query<(Entity, &IconText, &mut super::LiveText)>,
        Query<&ChildOf>,
    ),
) {
    if fresh.begun() {
        *last = None;
        sweeps.clear();
    }
    // Bound the sweep cache: a resolution / UI-scale change mints a fresh full set (`AdditiveCache`'s rule).
    if sweeps.len() > 4096 {
        sweeps.clear();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    // `standardHUD` moves the bar: redraw.
    if std::mem::replace(&mut *classic, assets.classic_hud()) != *classic {
        *last = None;
    }
    // The devotion window hides the buff bar (as in the real game). PORT: the engine's switch is
    // not decompiled.
    let icons: &[StatusIcon] = if panels.devotion { &[] } else { &vitals.status };
    let scale = assets.scale(size).to_bits();
    let same = last.as_ref().is_some_and(|l| l.same(size, scale, icons));
    let kept = super::retire_roots(
        &mut commands,
        roots.iter().map(|(e, f)| (e, f.0)),
        frame.0,
        !same,
        icons.is_empty(),
    );
    if same {
        for (s, mut img, mut vis) in &mut sweep_nodes {
            let Some(icon) = icons.get(s.index) else {
                continue;
            };
            if icon.sweep.0 > 0 {
                let h = sweep_handle(
                    &mut sweeps,
                    &mut assets.images,
                    s.size,
                    icon.sweep,
                    s.round,
                    &s.colors,
                );
                if img.image != h {
                    img.image = h;
                }
            }
            vis.set_if_neq(if icon.sweep.0 > 0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        // Only the kept root's texts: a retired root's despawn is queued (`retire_roots`).
        for (e, t, mut live) in &mut texts {
            if let Some(icon) = icons.get(t.0)
                && kept == Some(parents.root_ancestor(e))
            {
                live.set(e, icon_text(icon), &mut commands, &data, &mut assets);
            }
        }
        return;
    }
    *last = Some(BarLayout {
        size,
        scale,
        icons: icons
            .iter()
            .map(|i| {
                let (a, b, c, d) = BarLayout::key(i);
                (a.to_owned(), b, c, d)
            })
            .collect(),
    });
    built.0.clear();
    if icons.is_empty() {
        return;
    }
    let root = gd_ui_kit::spawn_frame_root(&mut commands, StatusRoot, frame.0, 1);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let Some(rec) = record(&ui) else { return };
    let colors = sweep_colors(&rec);
    let style = rec
        .str("cooldownFontStyle")
        .unwrap_or("records/ui/styles/text/style_skillcooldowntext.dbr")
        .to_owned();
    let (tx, ty) = (
        ui.sc(rec.f32("cooldownTextLocationX").unwrap_or(0.0)),
        ui.sc(rec.f32("cooldownTextLocationY").unwrap_or(0.0)),
    );
    built.0 = layout(&mut ui, icons);
    let lh = ui.line_height(&style);
    for (index, (icon, &r)) in icons.iter().zip(&built.0).enumerate() {
        if let Some((img, _)) = ui.image(&icon.icon) {
            ui.quad_stretch(img, r, Color::WHITE);
        }
        // The sweep's node exists while the icon does (hidden at step 0), so a sweep starts in place.
        let sweep = IconSweep {
            index,
            size: (r.w as u32, r.h as u32),
            round: icon.round,
            colors,
        };
        let h = sweep_handle(
            &mut sweeps,
            ui.images,
            sweep.size,
            (icon.sweep.0.max(1), icon.sweep.1),
            icon.round,
            &colors,
        );
        let e = ui.quad_stretch(h, r, Color::WHITE);
        let vis = if icon.sweep.0 > 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        ui.commands.entity(e).insert((sweep, vis));
        // RenderText2d at the location: the line's bottom there.
        let tint = if icon.text.as_ref().is_some_and(|t| t.1) {
            Color::WHITE
        } else {
            Color::srgb(1.0, 0.0, 0.0)
        };
        let at = super::TextAt::Line {
            style: style.clone(),
            at: Vec2::new(r.x + tx, r.y + ty - lh),
            centre: false,
            tint,
        };
        let e = ui.live_text(at, icon_text(icon));
        ui.commands.entity(e).insert(IconText(index));
    }
}

/// Hover tooltip of a buff-bar icon: the skill's tooltip
/// (`gd_sim::skill_tooltip::skill_lines`), centred below the icon and
/// clamped to the screen ([`super::tip::place_below`]), wrapped at the
/// sample line's width ([`super::tip::Wrap::Sample`]), shown through the
/// shared [`super::tip::TipCanvas`].
/// A covering window suppresses it: the pause menu, the map / journal
/// ([`super::skills::Above`]) and a registered window button / blocker in front
/// of the icon ([`super::widgets::frontmost`], as `rollover::hud_rollover`).
/// `GD_BUFF_HOVER=<n>` hovers the n-th icon (screenshots).
#[allow(clippy::too_many_arguments)]
pub fn buff_tooltip(
    mut fresh: gd_game_core::session::NewSession,
    hooks: Res<gd_game_core::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    (values, vitals): (Res<HudValues>, Res<super::HudVitals>),
    caster: Query<
        (
            &gd_game_core::combat::Combatant,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
        ),
        With<gd_game_core::player::Player>,
    >,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    mut canvas: super::tip::TipCanvas,
    built: Res<StatusLayout>,
    pause: Res<gd_game_core::pause::GamePause>,
    above: super::skills::Above,
    (button_sounds, gz): (
        Query<(&super::widgets::ButtonSound, &ChildOf, &InheritedVisibility)>,
        super::widgets::RootZ,
    ),
    mut shown: Local<Option<(String, UVec2, usize)>>,
) {
    if fresh.begun() {
        *shown = Default::default();
    }
    let (Ok(win), Ok((cb, links))) = (window.single(), caster.single()) else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let rects = &built.0;
    let forced = hooks.ui.buff_hover;
    // A covering window hides the bar: the pause menu, the map / journal (`skills::Above`) and a
    // registered window button / blocker in front of the icon (`widgets::frontmost`, as
    // `rollover::hud_rollover`). `UiCapture` alone would also hide it over the HUD itself (`hud::capture`).
    let cursor = super::cursor_px(&hooks, win).filter(|p| !pause.menu && !above.covers(*p));
    let front = cursor.and_then(|p| super::widgets::frontmost(p, &button_sounds, &gz));
    let in_front = |r: &URect| front.is_none_or(|f| f.rect == *r);
    let hover = {
        let fonts = &canvas.assets.fonts;
        match forced {
            Some(n) => vitals.status.get(n).zip(rects.get(n)),
            None => cursor.and_then(|p| {
                vitals
                    .status
                    .iter()
                    .zip(rects)
                    .find(|(_, r)| super::contains(r, p) && in_front(r))
            }),
        }
        .and_then(|(i, r)| {
            // `PlayerStatusIcons::GetRolloverText` kind 2: tagHUDComboCharge (class 46), the multiplier line
            // (49, when non-zero) and the time left (49; `…Precise` under 1000 ms). PORT: the
            // stock text files lack both combo tags; the engine then shows the tag name.
            if let Some((mult, ms)) = i.combo {
                use super::floating::format_text;
                use gd_game_core::floating::TextArg;
                use gd_sim::skill_tooltip::{TextLine, class};
                let line = |class, text| TextLine {
                    class,
                    text,
                    icon: None,
                };
                let mut lines = vec![line(
                    class::SKILL_NAME,
                    format_text(fonts, "tagHUDComboCharge", &[]),
                )];
                if mult != 0 {
                    lines.push(line(
                        class::SKILL_STATS_CURRENT,
                        format_text(
                            fonts,
                            "tagHUDComboChargeMultiplier",
                            &[TextArg::Num(mult as f32)],
                        ),
                    ));
                }
                let tag = if ms < 1000 {
                    "tagDotTimeRemainingPrecise"
                } else {
                    "tagDotTimeRemaining"
                };
                lines.push(line(
                    class::SKILL_STATS_CURRENT,
                    format_text(fonts, tag, &[TextArg::Num(ms as f32 * 0.001)]),
                ));
                let key = lines
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some((key, Some(lines), *r));
            }
            if let Some((ty, mag, ms)) = i.dot {
                let lines = dot_lines(fonts, ty, mag, ms);
                if lines.is_empty() {
                    return None;
                }
                let key = lines
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some((key, Some(lines), *r));
            }
            // `Skill_ChargePotion::CreateUITrackerText`: name and absorption only.
            if let (Some(absorb), Some(skill)) = (i.absorb, &i.tip) {
                let ctx = gd_sim::skill_tooltip::SkillTipCtx {
                    db: &canvas.data.db,
                    trees: &canvas.data.skill_trees(),
                    tags: &fonts.tags,
                    book: &values.skills,
                    player: None,
                    pet_bonuses: &[],
                };
                let mods = links.map_or(&[][..], |l| l.mods_of(skill));
                let lines = gd_sim::skill_tooltip::potion_tracker_lines(&ctx, skill, mods, absorb);
                let key = format!("{skill}|{absorb:.0}");
                return Some((key, Some(lines), *r));
            }
            i.tip.clone().map(|t| (t, None, *r))
        })
    };
    let key = hover
        .as_ref()
        .map(|(t, _, _)| (t.clone(), size, values.level as usize));
    if *shown == key {
        return;
    }
    *shown = key;
    canvas.clear(super::tip::TipOwner::Buff);
    let Some((skill, combo_lines, anchor)) = hover else {
        return;
    };
    let lines = combo_lines.unwrap_or_else(|| {
        // `Skill::CreateUITrackerText`: a skill with a
        // `trackerOverride` tag shows only its name (class 46) and that text (class 47); 13 stock records have one.
        if let Some(over) = canvas.data.db.get(&skill).and_then(|r| {
            r.str("trackerOverride")
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        }) {
            let fonts = &canvas.assets.fonts;
            let name = gd_sim::skills::profile_record(&canvas.data.db, &skill)
                .and_then(|p| p.str("skillDisplayName").map(str::to_owned))
                .map(|t| super::floating::format_text(fonts, &t, &[]))
                .unwrap_or_default();
            let line = |class, text: String| gd_sim::skill_tooltip::TextLine {
                class,
                text,
                icon: None,
            };
            let mut out = Vec::new();
            if !name.is_empty() {
                out.push(line(gd_sim::skill_tooltip::class::SKILL_NAME, name));
            }
            let text = super::floating::format_text(fonts, &over, &[]);
            if !text.is_empty() {
                out.push(line(gd_sim::skill_tooltip::class::SKILL_DESCRIPTION, text));
            }
            return out;
        }
        let ctx = gd_sim::skill_tooltip::SkillTipCtx {
            db: &canvas.data.db,
            trees: &canvas.data.skill_trees(),
            tags: &canvas.assets.fonts.tags,
            book: &values.skills,
            player: rules.as_ref().map(|r| (&*r.0, &*cb.profile)),
            pet_bonuses: &[],
        };
        // `PlayerStatusIcons::GetRolloverText` kind 0: the skill's heading (name 46, description 47); the parameter
        // block only for a skill with `trackerShowDetail` (14 stock records, the potion modifier / shrine buffs).
        let detail = canvas
            .data
            .db
            .get(&skill)
            .is_some_and(|r| r.bool("trackerShowDetail"));
        if detail {
            gd_sim::skill_tooltip::skill_detail_lines(&ctx, &skill)
        } else {
            gd_sim::skill_tooltip::skill_lines(&ctx, &skill, None)
                .into_iter()
                .filter(|l| {
                    matches!(
                        l.class,
                        gd_sim::skill_tooltip::class::SKILL_NAME
                            | gd_sim::skill_tooltip::class::SKILL_DESCRIPTION
                    )
                })
                .collect()
        }
    });
    if lines.is_empty() {
        return;
    }
    // `PlayerStatusIcons::GetRolloverText` → the HUD rollover window (`UISkillPane::HandleMouseEvent` hands the icon to
    // `RolloverWindow::SetRollover`, which copies the request, `RolloverWindow::CopyRequest`): wrap mode 1
    // (`RolloverRequest.wrapMode`) and placement mode 4 (`RolloverRequest.placement`) at the icon's rect
    // (`RolloverRequest.rect`).
    canvas.show_wrapped(
        super::tip::TipOwner::Buff,
        &lines,
        super::tip::Blank::DefaultLine,
        super::tip::Wrap::Sample,
        |w, h, screen, _| super::tip::place_below(anchor, w, h, screen),
    );
}

/// Hover tooltip of a hotbar slot (`gd_sim::skill_tooltip::hot_slot_lines`: the heading, the current
/// level and the usability line, `HotSlotOptionSkill::GetRolloverText`; the evade and potion
/// slots the same text of their default skill, `default_hot_slot_lines`). The usability line reads
/// `Skill.availability`, which `Skill::SetAvailability` keeps every frame (`gd_sim::skill_tooltip::hot_state`: the
/// energy against `energy_cost` unless `ignores_energy`, the cooldown, the worn weapons by `SkillLinks::weapons_allow`).
/// PORT: placed centred above the slot, bottom edge just over it.
/// A covering window suppresses it: the pause menu, the map / journal
/// ([`super::skills::Above`]) and a registered window button / blocker in front
/// of the slot ([`super::widgets::frontmost`], as `rollover::hud_rollover`).
/// `GD_HOT_HOVER=x,y` (physical px) fakes the cursor.
#[allow(clippy::too_many_arguments)]
pub fn hotbar_tooltip(
    mut fresh: gd_game_core::session::NewSession,
    hooks: Res<gd_game_core::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    (values, hud_layout): (Res<HudValues>, Res<super::hud::HudLayout>),
    caster: Query<
        (
            &gd_game_core::casting::Caster,
            &gd_game_core::combat::Combatant,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            Option<&gd_game_core::player::Loadout>,
        ),
        With<gd_game_core::player::Player>,
    >,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    mut canvas: super::tip::TipCanvas,
    pause: Res<gd_game_core::pause::GamePause>,
    above: super::skills::Above,
    (button_sounds, gz): (
        Query<(&super::widgets::ButtonSound, &ChildOf, &InheritedVisibility)>,
        super::widgets::RootZ,
    ),
    mut shown: Local<
        Option<(
            String,
            gd_sim::skill_tooltip::HotState,
            UVec2,
            usize,
            Option<u32>,
        )>,
    >,
) {
    if fresh.begun() {
        *shown = Default::default();
    }
    use gd_game_core::casting::{ENERGY_POTION, HEALTH_POTION, SLOT_LEFT, SLOT_RIGHT, SLOT_ROW2};
    use gd_sim::skill_tooltip::{SkillTipCtx, hot_state};
    let (Ok(win), Ok((c, cb, links, loadout))) = (window.single(), caster.single()) else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    // A covering window hides the slots: the pause menu, the map / journal (`skills::Above`) and a
    // registered window button / blocker in front of the slot (`widgets::frontmost`, as
    // `rollover::hud_rollover`). `UiCapture` alone would also hide them over the HUD itself (`hud::capture`).
    let cursor = hooks
        .ui
        .hot_hover
        .or_else(|| super::cursor_px(&hooks, win))
        .filter(|p| !pause.menu && !above.covers(*p));
    let front = cursor.and_then(|p| super::widgets::frontmost(p, &button_sounds, &gz));
    let in_front = |r: &URect| front.is_none_or(|f| f.rect == *r);

    // The slot under the cursor (the rects are the built HUD's, `hud::HudLayout`), then its skill.
    let row = values.hotbar_row * SLOT_ROW2;
    let hover = cursor.and_then(|p| {
        let over = |f: &str| {
            hud_layout
                .rect(f)
                .filter(|r| super::contains(r, p) && in_front(r))
        };
        [
            ("hudEvadeButton", gd_game::evade::EVADE),
            ("hudPriPotionButton", HEALTH_POTION),
            ("hudSecPotionButton", ENERGY_POTION),
        ]
        .into_iter()
        .find_map(|(f, sk)| over(f).map(|r| (sk.to_owned(), r)))
        .or_else(|| {
            super::hud::HOT_SLOTS
                .into_iter()
                .enumerate()
                .map(|(i, f)| (f, i + row))
                .chain([
                    ("hudPriSkillButton", SLOT_LEFT),
                    ("hudSecSkillButton", SLOT_RIGHT),
                ])
                .find_map(|(f, n)| {
                    let sk = c.slots.get(n)?.as_ref()?;
                    over(f).map(|r| (sk.clone(), r))
                })
        })
    });
    let evade = hover
        .as_ref()
        .is_some_and(|(sk, _)| sk == gd_game::evade::EVADE);
    let potion = hover
        .as_ref()
        .is_some_and(|(sk, _)| sk == HEALTH_POTION || sk == ENERGY_POTION);
    // An unlearned slotted skill that worn gear grants (`Skill.grantedByItem`) or a form grants for a while: its level.
    let granted = hover
        .as_ref()
        .filter(|(sk, _)| !evade && !potion && values.skills.level(sk) == 0)
        .and_then(|(sk, _)| {
            let l = loadout?;
            let gear = gd_sim::skills::granted_level(
                &gd_sim::skills::item_skill_grants(&canvas.data.db, &l.items),
                sk,
            );
            let form = l
                .granted
                .iter()
                .filter(|(g, _)| g.eq_ignore_ascii_case(sk))
                .map(|(_, lv)| *lv)
                .max()
                .unwrap_or(0);
            Some(gear.max(form)).filter(|l| *l > 0)
        });
    let item_skill = granted.is_some();
    let state = hover.as_ref().map(|(sk, _)| {
        let ctx = SkillTipCtx {
            db: &canvas.data.db,
            trees: &canvas.data.skill_trees(),
            tags: &canvas.assets.fonts.tags,
            book: &values.skills,
            player: rules.as_ref().map(|r| (&*r.0, &*cb.profile)),
            pet_bonuses: &[],
        };
        let db = &canvas.data.db;
        let ignores = db.get(sk).is_some_and(|r| {
            gd_sim::skill_tooltip::ignores_energy(
                &r.class,
                loadout.is_some_and(|l| l.skill_active(db, sk)),
            )
        });
        let energy_ok = evade
            || potion
            || ignores
            || c.energy >= gd_sim::skill_tooltip::energy_cost(&ctx, sk, item_skill);
        let cooling = c.cooldowns.get(sk).is_some_and(|l| *l > 0.0);
        let weapons_ok = match (links, canvas.data.db.get(sk)) {
            (Some(l), Some(rec)) => l.weapons_allow(&canvas.data.db, rec, sk),
            _ => true,
        };
        hot_state(energy_ok, cooling, weapons_ok)
    });
    let key = hover
        .as_ref()
        .zip(state)
        .map(|((sk, _), st)| (sk.clone(), st, size, values.level as usize, granted));
    if *shown == key {
        return;
    }
    *shown = key;
    canvas.clear(super::tip::TipOwner::Hotbar);
    let (Some((skill, anchor)), Some(state)) = (hover, state) else {
        return;
    };
    // PORT: the evade text (real HUD) shows the record's base numbers: no player scaling.
    let ctx = SkillTipCtx {
        db: &canvas.data.db,
        trees: &canvas.data.skill_trees(),
        tags: &canvas.assets.fonts.tags,
        book: &values.skills,
        player: rules
            .as_ref()
            .filter(|_| !evade)
            .map(|r| (&*r.0, &*cb.profile)),
        pet_bonuses: &[],
    };
    let lines = if evade || potion {
        gd_sim::skill_tooltip::default_hot_slot_lines(&ctx, &skill, state)
    } else {
        gd_sim::skill_tooltip::hot_slot_lines(&ctx, &skill, state, granted)
    };
    if lines.is_empty() {
        return;
    }
    canvas.show(
        super::tip::TipOwner::Hotbar,
        &lines,
        super::tip::Blank::DefaultLine,
        |w, h, screen, gap| {
            (
                (anchor.x + (anchor.w - w) * 0.5).clamp(0.0, (screen.x - w).max(0.0)),
                (anchor.y - h - gap * 0.25).clamp(0.0, (screen.y - h).max(0.0)),
            )
        },
    );
}
