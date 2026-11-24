//! Floating world texts: game event 27 (`EventManager::Send(.., 0x1B)`),
//! which the game window turns into a rising text at a world position
//! (drawn by `grimdawn::ui::floating`).
//!
//! The event: object id, world position, font style record, text, flags,
//! scale (constructor `FloatingTextEvent::FloatingTextEvent`: flags 6, scale 1). Senders (Game.dll,
//! every one only for the main player or its pets):
//! - money: `Character::TakeBonus`: the amount (raised by the
//!   gold bonus %, attribute 56) at the character with its object id,
//!   `GiveMoneyFormat` ("+{%d0} Iron"), gameengine.dbr `moneyPickupStyle`,
//!   flags 28, scale 1 ([`FloatingText::money`]);
//! - pickups (`OnPickup`, the main player only; [`FloatingText::pickup`]):
//!   `ItemRelic` (components) / `ItemCharm` and `QuestItem` the item's name
//!   (`SimpleStringFormat` of its `description` tag, colour tags stripped)
//!   in `relicPickupStyle`; `OneShot_Potion` and its subclass
//!   `OneShot_Scroll` (elixirs, potions) the same in
//!   `healthPotionPickupStyle`; `OneShot_PotionHealth` / `_PotionMana` (only
//!   unused `_oldpotion` records) `tagHUD_PotionHealth` / `tagHUD_PotionEnergy`
//!   in `health…` / `manaPotionPickupStyle`; all at the player with its
//!   object id, flags 28, scale 1; other items say nothing;
//! - experience: `GameEngine::HandleExperienceNotification`
//!   (from `PlayerManagerServer::HandleBonus` on a kill): only for a Hero
//!   (classification 2: "Enemy Hero Killed") or Boss / Quest / SuperBoss
//!   (3..5: "Enemy Boss Killed") kill with experience, `tagXPAwarded`
//!   ("{%d0} XP Awarded") at the player with its object id,
//!   `style_experiencebarpopup.dbr`, flags 6 ([`FloatingText::experience`]);
//!   the cinematic text (type 5, `tagHeroKilled` / `tagBossKilled` over the same line) goes out with it
//!   ([`crate::progress::kill_experience`]);
//! - combat numbers: `CombatManager::TakeAttack` when the
//!   `displayDamage` option (on by default: `Options::Options`,
//!   Engine.dll; [`gd_game_core::options::GameOptions`]) is on and the attacker is the main player or a monster whose
//!   `GetLeader` is the main player (its pets); style names below are the
//!   player's, the `pet…` variants for a pet ([`combat_text`]):
//!   - fumble `tagFumble` at the attacker's `HeadEffect` point; dodge
//!     `tagDodge`, deflect `tagDeflect`, miss `tagMiss` at the defender's;
//!     each in `fumble/dodge/deflect/missStyle`, flags 70, no object;
//!   - a shield block: no damage `tagBlock` (`blockStyle`), else `HitFormat`
//!     ("{%.0f0}") in `partialBlockStyle`; flags 70, scale 1;
//!   - damage < 1: nothing; a normal hit `HitFormat` in `hitStyle`, flags 70,
//!     scale `Player::RegisterCombatTextHit(damage)`;
//!   - a critical (multiplier > 1) `CriticalHitFormatMult` ("{%.0f0}
//!     (x{%.2f1})") with the `critMultipliers` option (off by
//!     default), else `CriticalHitFormat` ("{%.0f0}"), with
//!     the defender's object id, flags 133, the style `critStyle[tier − 1]`
//!     (`GameEngine::GetCritTextStyle`: an index past the list → the first),
//!     scale `RegisterCombatTextCrit(damage)`;
//!   - a pet's hit or critical takes `petHitStyle` / `petCritStyle` and its
//!     leader's (the player's) `RegisterCombatTextCrit(damage)` as scale.
//!
//!   `Player::RegisterCombatTextHit` / `…Crit`
//!   keep a running sum and count of the damage (one pair each) and return
//!   0.8 + 0.4 × (clamp(damage / mean, 0.5, 1.5) − 0.5) ([`CombatTextMeans`]).
//!   Retaliation (style 3) and reflection (style 6) hits run the same code
//!   with the retaliating / reflecting character as attacker, so the
//!   player's and its pets' show as plain hits on the struck foe
//!   ([`crate::combat::CounterHit`]).
//!
//!   Invulnerable (`TakeAttack`): a living attacker whose racial
//!   list one of the defender's `CombatFilter`s (`Character.combatFilters`) refuses
//!   (`ProvidesInvulnerability`: only `Skill_BuffSelfInvulnerable` while
//!   active, every race or its own list; every other filter class says no)
//!   deals nothing, and `tagInvulnerable` rises at the defender in
//!   `(pet)InvulnerableStyle`, flags 70. PORT: not sent: stock data has no
//!   `Skill_BuffSelfInvulnerable` record (only its template, listed in
//!   `skillTemplates`), so it never shows in vanilla.
//! - DoT numbers: `DurationDamageManager::Update`
//!   once a second per character with ≥ 1 damage of the main player's own
//!   DoTs ([`gd_game_core::afflictions::DotNumbers`]), same option: at the victim's
//!   `HeadEffect`, no object, flags 6, scale `RegisterCombatTextHit(damage)`
//!   (even for a critical); the largest multiplier ≤ 1 `HitFormat` in
//!   `hitStyle`, else `CriticalHitFormat(Mult)` (damage, multiplier) in
//!   `critStyle[0]` ([`dot_text`]).
//!
//!   PORT: `ProjectileBase::ApplyFumbleDamage` is not ported.
use bevy::prelude::*;
use gd_formats::vfs::Database;
use gd_game_core::floating::TextArg;
use gd_sim::combat::MissReason;

/// Event 27's flags for a money pickup (`Character::TakeBonus`): 0x04 fade
/// out, 0x08 queue behind the object's pending texts, 0x10 arc away
/// sideways (`grimdawn::ui::floating`).
pub const MONEY_PICKUP_FLAGS: u32 = 28;
/// Combat numbers (`TakeAttack`): 0x40 random direction, 0x04 fade, 0x02 rise.
pub const COMBAT_FLAGS: u32 = 70;
/// Critical hits: 0x80 per-object offsets, 0x04 fade, 0x01 grow and shrink.
pub const CRIT_FLAGS: u32 = 133;
/// The experience popup: 0x04 fade, 0x02 rise.
pub const EXPERIENCE_FLAGS: u32 = 6;
const EXPERIENCE_STYLE: &str = "records/ui/styles/text/style_experiencebarpopup.dbr";

pub struct FloatingTextPlugin;

impl Plugin for FloatingTextPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FloatingText>().add_systems(
            Update,
            combat_texts
                .after(gd_game_core::combat::CombatSet)
                .after(gd_game_core::afflictions::AfflictionStage::Tick),
        ); // reads the AttackMade / CounterHit / DotNumbers those write this frame
    }
}

/// A floating text request (event 27).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct FloatingText {
    /// The object it rises from (its "HeadEffect" point), if any.
    pub entity: Option<Entity>,
    /// World position (where it starts; the object's point replaces it once
    /// shown).
    pub position: Vec3,
    /// Text tag and its `{%d0}`, `{%.0f1}`, `{%s2}`… arguments.
    pub tag: String,
    pub args: Vec<TextArg>,
    /// Font style record.
    pub style: String,
    pub flags: u32,
    pub scale: f32,
}

fn engine_str(db: &Database, field: &str) -> String {
    db.get("records/game/gameengine.dbr")
        .and_then(|r| r.str(field).map(str::to_owned))
        .unwrap_or_default()
}

impl FloatingText {
    /// "+{%d0} Iron" over `entity` (`Character::TakeBonus`).
    pub fn money(db: &Database, entity: Entity, position: Vec3, amount: u64) -> Self {
        Self {
            entity: Some(entity),
            position,
            tag: "GiveMoneyFormat".into(),
            args: vec![TextArg::Num(amount as f32)],
            style: engine_str(db, "moneyPickupStyle"),
            flags: MONEY_PICKUP_FLAGS,
            scale: 1.0,
        }
    }

    /// The pickup text of an item of record `item`, if its class has one
    /// (module docs: the `OnPickup` overrides).
    pub fn pickup(db: &Database, entity: Entity, position: Vec3, item: &str) -> Option<Self> {
        let rec = db.get(item)?;
        let name = || {
            rec.str("description")
                .filter(|d| !d.is_empty())
                .map(|d| ("SimpleStringFormat", vec![TextArg::Tag(d.to_owned())]))
        };
        let ((tag, args), style) = match rec.class.as_str() {
            "ItemRelic" | "ItemCharm" | "QuestItem" => (name()?, "relicPickupStyle"),
            "OneShot_Potion" | "OneShot_Scroll" => (name()?, "healthPotionPickupStyle"),
            "OneShot_PotionHealth" => (
                ("tagHUD_PotionHealth", Vec::new()),
                "healthPotionPickupStyle",
            ),
            "OneShot_PotionMana" => (("tagHUD_PotionEnergy", Vec::new()), "manaPotionPickupStyle"),
            _ => return None,
        };
        Some(Self {
            entity: Some(entity),
            position,
            tag: tag.into(),
            args,
            style: engine_str(db, style),
            flags: MONEY_PICKUP_FLAGS,
            scale: 1.0,
        })
    }

    /// "{%d0} XP Awarded" over the player for a hero or boss kill
    /// (`HandleExperienceNotification`); None for other monsters.
    pub fn experience(
        db: &Database,
        player: Entity,
        position: Vec3,
        monster: &str,
        xp: u64,
    ) -> Option<Self> {
        let class = db
            .get(monster)?
            .str("monsterClassification")
            .unwrap_or("Common")
            .to_owned();
        (xp > 0 && matches!(class.as_str(), "Hero" | "Boss" | "Quest" | "SuperBoss")).then(|| {
            Self {
                entity: Some(player),
                position,
                tag: "tagXPAwarded".into(),
                args: vec![TextArg::Num(xp as f32)],
                style: EXPERIENCE_STYLE.into(),
                flags: EXPERIENCE_FLAGS,
                scale: 1.0,
            }
        })
    }
}

/// `Player::RegisterCombatTextHit` / `…Crit`: running damage sums and
/// counts (`Player.combatTextHitSum` .. `Player.combatTextCritCount`) that size the numbers.
#[derive(Component, Default, Clone, Debug)]
pub struct CombatTextMeans {
    hit: (f32, f32),
    crit: (f32, f32),
}

impl CombatTextMeans {
    fn register(m: &mut (f32, f32), damage: f32) -> f32 {
        m.0 += damage;
        m.1 += 1.0;
        (((damage / (m.0 / m.1)).clamp(0.5, 1.5) - 0.5) * 0.400_000_04) + 0.8
    }
    pub fn hit(&mut self, damage: f32) -> f32 {
        Self::register(&mut self.hit, damage)
    }
    pub fn crit(&mut self, damage: f32) -> f32 {
        Self::register(&mut self.crit, damage)
    }
}

/// Where a combat text starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    Attacker,
    Defender,
}

/// The combat number of one attack outcome (module docs), without its
/// position: (text, at). `pet`: a pet of the player attacked; `means`: the
/// player's [`CombatTextMeans`]; `mult`: the `critMultipliers` option.
pub fn combat_text(
    db: &Database,
    made: &crate::combat::AttackMade,
    pet: bool,
    means: &mut CombatTextMeans,
    mult: bool,
) -> Option<(FloatingText, At)> {
    let style = |name: &str| {
        engine_str(
            db,
            &if pet {
                format!("pet{}{}", name[..1].to_ascii_uppercase(), &name[1..])
            } else {
                name.to_owned()
            },
        )
    };
    let plain = |tag: &str, style: String| FloatingText {
        entity: None,
        position: Vec3::ZERO,
        tag: tag.into(),
        args: Vec::new(),
        style,
        flags: COMBAT_FLAGS,
        scale: 1.0,
    };
    let damage = made.total;
    if let Some(m) = made.miss {
        return Some(match m {
            MissReason::Fumble => (plain("tagFumble", style("fumbleStyle")), At::Attacker),
            MissReason::Dodge => (plain("tagDodge", style("dodgeStyle")), At::Defender),
            MissReason::Deflect => (plain("tagDeflect", style("deflectStyle")), At::Defender),
            MissReason::Missed => (plain("tagMiss", style("missStyle")), At::Defender),
        });
    }
    if made.blocked {
        if damage <= 0.0 {
            return Some((plain("tagBlock", style("blockStyle")), At::Defender));
        }
        let mut t = plain("HitFormat", style("partialBlockStyle"));
        t.args = vec![TextArg::Num(damage), TextArg::Num(made.multiplier)];
        return Some((t, At::Defender));
    }
    if damage < 1.0 {
        return None;
    }
    let args = vec![TextArg::Num(damage), TextArg::Num(made.multiplier)];
    if made.multiplier <= 1.0 {
        let scale = if pet {
            means.crit(damage)
        } else {
            means.hit(damage)
        };
        return Some((
            FloatingText {
                args,
                scale,
                ..plain("HitFormat", style("hitStyle"))
            },
            At::Defender,
        ));
    }
    let style = if pet {
        style("critStyle")
    } else {
        let i = made.tier.wrapping_sub(1) as usize;
        db.get("records/game/gameengine.dbr")
            .and_then(|r| {
                r.strs_iter("critStyle")
                    .nth(i)
                    .or_else(|| r.strs_iter("critStyle").next())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default()
    };
    let scale = means.crit(damage);
    Some((
        FloatingText {
            entity: Some(made.target),
            args,
            scale,
            flags: CRIT_FLAGS,
            ..plain(
                if mult {
                    "CriticalHitFormatMult"
                } else {
                    "CriticalHitFormat"
                },
                style,
            )
        },
        At::Defender,
    ))
}

/// The once-a-second DoT number (module docs).
pub fn dot_text(
    db: &Database,
    damage: f32,
    multiplier: f32,
    means: &mut CombatTextMeans,
    mult: bool,
) -> FloatingText {
    let scale = means.hit(damage);
    let (tag, style) = if multiplier <= 1.0 {
        ("HitFormat", engine_str(db, "hitStyle"))
    } else {
        let first = db
            .get("records/game/gameengine.dbr")
            .and_then(|r| r.strs_iter("critStyle").next().map(|s| s.to_string()))
            .unwrap_or_default();
        (
            if mult {
                "CriticalHitFormatMult"
            } else {
                "CriticalHitFormat"
            },
            first,
        )
    };
    let args = vec![
        TextArg::Num(damage),
        TextArg::Num(if multiplier <= 1.0 { 1.0 } else { multiplier }),
    ];
    FloatingText {
        entity: None,
        position: Vec3::ZERO,
        tag: tag.into(),
        args,
        style,
        flags: EXPERIENCE_FLAGS,
        scale,
    }
}

/// A character's `HeadEffect` attach point in the world, else 1.6 m up.
pub fn head_point(tf: &Transform, ch: Option<&gd_game_core::character::Character>) -> Vec3 {
    ch.and_then(|c| gd_game_core::casting::attach_point_bind(&c.body, "HeadEffect"))
        .map_or(tf.translation + Vec3::Y * 1.6, |l| tf.transform_point(l))
}

/// The combat numbers of the player's and its pets' attacks, retaliation /
/// reflection hits and DoTs this frame.
#[allow(clippy::too_many_arguments)]
fn combat_texts(
    mut commands: Commands,
    (mut made, mut counters, mut dots): (
        MessageReader<crate::combat::AttackMade>,
        MessageReader<crate::combat::CounterHit>,
        MessageReader<gd_game_core::afflictions::DotNumbers>,
    ),
    data: Res<gd_game_core::data::GameData>,
    players: Query<Entity, With<gd_game_core::player::Player>>,
    pets: Query<&crate::pets::Pet>,
    mut means: Query<&mut CombatTextMeans>,
    chars: Query<(&Transform, Option<&gd_game_core::character::Character>)>,
    mut texts: MessageWriter<FloatingText>,
    options: Res<gd_game_core::options::GameOptions>,
) {
    if made.is_empty() && counters.is_empty() && dots.is_empty() {
        return;
    }
    let player = players.single().ok();
    let Some(player) = player.filter(|_| options.flag("displayDamage")) else {
        made.clear();
        counters.clear();
        dots.clear();
        return;
    };
    let mult = options.flag("critMultipliers");
    let mut fresh = None;
    let counter_made = counters.read().map(|c| crate::combat::AttackMade {
        attacker: c.attacker,
        target: c.target,
        skill: None,
        critical: c.hit.critical,
        weapon: false,
        melee: false,
        miss: c.hit.miss,
        blocked: c.hit.blocked,
        multiplier: c.hit.multiplier,
        tier: c.hit.tier,
        total: c.hit.total,
        offensive_ability: 0.0,
        defensive_ability: 0.0,
    });
    let all: Vec<crate::combat::AttackMade> = made.read().cloned().chain(counter_made).collect();
    for m in &all {
        let pet = pets.get(m.attacker).is_ok_and(|p| p.owner == player);
        if m.attacker != player && !pet {
            continue;
        }
        let mut own = means.get_mut(player).ok();
        let mine = match own.as_deref_mut() {
            Some(x) => x,
            None => fresh.get_or_insert_with(CombatTextMeans::default),
        };
        let Some((mut t, at)) = combat_text(&data.db, m, pet, mine, mult) else {
            continue;
        };
        let Ok((tf, ch)) = chars.get(if at == At::Attacker {
            m.attacker
        } else {
            m.target
        }) else {
            continue;
        };
        t.position = head_point(tf, ch);
        texts.write(t);
    }
    for d in dots.read() {
        let Ok((tf, ch)) = chars.get(d.target) else {
            continue;
        };
        let mut own = means.get_mut(player).ok();
        let mine = match own.as_deref_mut() {
            Some(x) => x,
            None => fresh.get_or_insert_with(CombatTextMeans::default),
        };
        let mut t = dot_text(&data.db, d.damage, d.multiplier, mine, mult);
        t.position = head_point(tf, ch);
        texts.write(t);
    }
    if let Some(f) = fresh {
        commands.entity(player).try_insert(f);
    }
}
