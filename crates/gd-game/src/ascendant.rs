//! The ascendant altar as gameplay: using it, activating it with formula
//! reagents (the session's formulas, [`AltarState::formulas`]) and the Dread
//! spawns it enables. The game shows the altar and its offering menu.
//!
//! Using the altar ([`Altar`], level object `AscendantAltar`): a click on it is the player's interact order
//! (`player_orders::InteractTarget`), which walks beside it; [`use_altar`] uses it once the player is within
//! the use range (`player_orders::usable_range`). It needs the `ascendantToken`
//! ([`may_use_altar`]) and opens the altar window ([`AltarOpen`]) only while
//! the altar is dormant (`AscendantAltar::RequestToUse`: the window
//! opens only in the dormant states; the activation lasts the session,
//! the installed formulas are cleared only by
//! `GameEngine::ClearAltarFormulas`, called from `ExitPlayingMode` and the
//! engine's destructor, and `AscendantAltar::UpdateSelf` has no timer).
use crate::combat::{MonsterAi, MonsterKilled};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::ascendant::{AltarState, Offering, formula_levels, reagent_quantity};
use gd_game_core::combat::Combatant;
use gd_game_core::player::Player;

#[derive(Resource, Default)]
pub struct DreadCount(pub u32, pub Option<Entity>);

/// The Dread (see the module docs).
#[allow(clippy::too_many_arguments)]
pub fn dread(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    mut killed: MessageReader<MonsterKilled>,
    mut count: ResMut<DreadCount>,
    rules: Res<gd_game_core::rules::CombatRules>,
    session: crate::session::Session,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    nav: Res<Nav>,
    mut assets: crate::creature::Spawner,
    players: Query<(Entity, &Transform, &Combatant), With<Player>>,
    credit: crate::combat::KillCredit,
    alive: Query<(), (With<Combatant>, Without<gd_game_core::combat::Dead>)>,
) {
    let Ok((player, ptf, pcb)) = players.single() else {
        killed.clear();
        return;
    };
    let kills = killed.read().filter(|k| credit.of(k)).count() as u32;
    if kills == 0 || count.1.is_some_and(|e| alive.get(e).is_ok()) {
        return;
    }
    let db = assets.data.db.clone();
    let disable = db
        .get("records/game/gameengine.dbr")
        .and_then(|r| r.str("superBossDisableToken").map(str::to_owned))
        .unwrap_or_default();
    let disabled = !disable.is_empty()
        && rt
            .as_ref()
            .is_some_and(|rt| rt.story.borrow().player.tokens.contains(&disable));
    let (st, mode) = (session.ascendant(), session.mode());
    if disabled && st.super_boss_chance <= 0.0 && st.super_boss_spawn <= 0.0 {
        return;
    }
    count.0 += kills;
    let need = gd_sim::ascendant::super_boss_kills(&db, st);
    // `GD_DREAD_KILLS` (hooks.rs): that many kills summon it for sure.
    #[cfg(feature = "dev")]
    let (need, forced) = (
        hooks.gameplay.dread_kills.unwrap_or(need),
        hooks.gameplay.dread_kills.is_some(),
    );
    #[cfg(not(feature = "dev"))]
    let forced = false;
    let chance = gd_sim::ascendant::super_boss_chance(&db, st, mode);
    if count.0 < need || chance <= 0.0 {
        return;
    }
    let roll = gd_game_core::rng::f32() * 100.0;
    debug!("ascendant: dread roll {roll:.2} vs {chance:.2} %");
    if roll > chance && !forced {
        return;
    }
    let Some(record) = gd_sim::ascendant::super_boss_record(&db, st, mode) else {
        return;
    };
    let dir = Vec3::new(
        gd_game_core::rng::f32() * 2.0 - 1.0,
        0.0,
        gd_game_core::rng::f32() * 2.0 - 1.0,
    )
    .normalize_or(Vec3::X);
    let Some(at) = [20.0, 15.0, 10.0, 5.0].iter().find_map(|d| {
        let p = ptf.translation + dir * *d;
        nav.0
            .ground((p + Vec3::Y * 2.0).to_array())
            .map(|g| Vec3::new(p.x, g, p.z))
    }) else {
        return;
    };
    let Some(rec) = db.get(&record) else { return };
    let level = pcb.profile.level.clamp(
        rec.i32("minLevel").unwrap_or(1).max(1),
        rec.i32("maxLevel").unwrap_or(i32::MAX).max(1),
    );
    // Facing the player: +Z forward (`character::facing`), `looking_to` aims −Z.
    let tf = Transform::from_translation(at).looking_to(
        at - Vec3::new(ptf.translation.x, at.y, ptf.translation.z),
        Vec3::Y,
    );
    let Some(e) = crate::creature::spawn(&mut commands, &mut assets, tf, &record, 9500) else {
        return;
    };
    let Some(prof) =
        gd_sim::actor::monster(&db, &rules.0, &record, level, gd_sim::actor::Side::Monster)
    else {
        return;
    };
    let mut ai = MonsterAi::from_record(&assets.data, &record, at);
    ai.aggro = true;
    ai.add_anger(player, 1000.0);
    // `OnCreatureDeath`: `EnableSpawnAnimation` before `FastSpawnEntity`.
    ai.enable_spawn_animation();
    commands.entity(e).insert((
        gd_game_core::monsters::Monster {
            record: record.clone(),
            level,
            slot: (usize::MAX, 0, 0),
        },
        Combatant::new(
            std::sync::Arc::new(prof),
            gd_game_core::combat::get_radius(Some(rec), 0.5),
        ),
        ai,
    ));
    *count = DreadCount(0, Some(e));
    info!(
        "ascendant: {} ({}) has come, level {level}",
        record.rsplit('/').next().unwrap_or(""),
        rec.str("description").unwrap_or("")
    );
}

/// Activates the altar with the reagents `placed` in its boxes (the centre
/// first): each box needs its formula's `reagentBaseQuantity` in the bags,
/// which are taken; each formula installs at the level of the boxes holding
/// its reagent (at the session's `mode`). `Ok`: the formulas and levels;
/// `Err`: why not. [`apply_altar`] hands the formulas to the monster rules
/// and the story.
pub fn activate_altar(
    db: &gd_formats::vfs::Database,
    mode: gd_rules::mode::Mode,
    placed: &[String],
    inv: &mut gd_game_core::inventory::Inventory,
    state: &mut AltarState,
) -> Result<Vec<(String, u32)>, String> {
    let mut need: Vec<(String, u32)> = Vec::new();
    for r in placed {
        let q = reagent_quantity(db, r);
        match need.iter_mut().find(|n| n.0 == *r) {
            Some(n) => n.1 += q,
            None => need.push((r.clone(), q)),
        }
    }
    if placed.is_empty() {
        return Err("nothing offered".to_owned());
    }
    if let Some((r, _)) = need.iter().find(|(r, q)| inv.count_by_record(r) < *q) {
        return Err(format!("not enough {}", r.rsplit('/').next().unwrap_or("")));
    }
    for (r, q) in &need {
        inv.remove_by_record(r, *q);
    }
    let st = &mut state.formulas;
    let levels = formula_levels(db, placed);
    for (f, l) in &levels {
        st.install(db, mode, f, *l);
    }
    info!(
        "ascendant: altar activated: {:?}; gold +{:.0}%, xp +{:.0}%, rep +{:.0}%, trove +{:.0}%, totem +{:.0}%, dread +{:.0}%, {} monster paks",
        levels
            .iter()
            .map(|(f, l)| format!("{} L{l}", f.rsplit('/').next().unwrap_or("")))
            .collect::<Vec<_>>(),
        st.gold * 100.0,
        st.experience * 100.0,
        st.reputation * 100.0,
        st.trove * 100.0,
        st.totem * 100.0,
        st.super_boss_chance * 100.0,
        st.monster_paks.len()
    );
    state.activated = true;
    state.reagents = Offering::default();
    for r in placed {
        state.reagents.place(db, r);
    }
    Ok(levels)
}

pub struct AscendantPlugin;

impl Plugin for AscendantPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DreadCount>()
            .init_resource::<AltarState>()
            .add_message::<AltarOpen>()
            .add_systems(
                Update,
                use_altar
                    .in_set(AscendantStage::Use)
                    .after(gd_game_core::player::PlayerSet)
                    .before(AscendantStage::Altar),
            )
            .add_message::<AltarOffer>()
            .add_message::<AltarAnswer>()
            .add_systems(
                Update,
                offer_to_altar
                    .run_if(resource_exists::<gd_game_core::inventory::Inventory>)
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                (
                    dread.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    apply_altar
                        .in_set(AscendantStage::Altar)
                        .before(gd_game_core::story::StoryStage::Tick)
                        .before(crate::monsters::MonsterStage::Proxies),
                ),
            );
    }
}

/// The altar window's activate button (`SendAltarOfferCmd`): activate with the reagents `placed`
/// ([`activate_altar`]). [`offer_to_altar`] applies it and answers with an
/// [`AltarAnswer`].
#[derive(Message, Clone, Debug)]
pub struct AltarOffer {
    pub placed: Vec<String>,
    /// The `GD_ALTAR_OFFER` hook: these reagents are added to the bags first.
    pub provide: Vec<String>,
}

/// The result of an [`AltarOffer`]: the installed levels or why not.
#[derive(Message, Clone, Debug)]
pub struct AltarAnswer(pub Result<Vec<(String, u32)>, String>);

fn offer_to_altar(
    mut msgs: MessageReader<AltarOffer>,
    mut answers: MessageWriter<AltarAnswer>,
    data: Res<gd_game_core::data::GameData>,
    mode: Res<gd_game_core::session::GameMode>,
    mut inv: ResMut<gd_game_core::inventory::Inventory>,
    mut altar: ResMut<AltarState>,
) {
    for m in msgs.read() {
        // Activated once per session: a second press must not re-take reagents nor stack the bonuses.
        if altar.activated {
            answers.write(AltarAnswer(Err("the altar is already activated".into())));
            continue;
        }
        for r in &m.provide {
            inv.add_to_bags(gd_formats::save::item::Item {
                base_name: r.clone(),
                stack_count: 1,
                seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
                ..Default::default()
            });
        }
        answers.write(AltarAnswer(activate_altar(
            &data.db, mode.0, &m.placed, &mut inv, &mut altar,
        )));
    }
}

/// An Ascendant Altar in the world (module doc).
#[derive(Component, Clone, Debug)]
pub struct Altar {
    pub record: String,
    /// `Actor::GetRadius` (`combat::get_radius`): the use range's and the hover pick's.
    pub radius: f32,
}

/// Spawns the altar's gameplay component (the dressing seam: grimdawn adds its model on `Added<Altar>`). The
/// radius is its `Actor::GetRadius` (`combat::get_radius`, the engine's `actorRadius` default when absent).
pub fn spawn_altar(
    commands: &mut Commands,
    db: &gd_formats::vfs::Database,
    tf: Transform,
    record: &str,
) -> Option<Entity> {
    let rec = db.get(record)?;
    let radius = gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS);
    Some(
        commands
            .spawn((
                tf,
                GlobalTransform::from(tf),
                Altar {
                    record: record.to_owned(),
                    radius,
                },
                crate::player_orders::Interactable::fixed(radius),
                Name::new("ascendant altar"),
            ))
            .id(),
    )
}

/// The answer to a use of the dormant altar: the game opens its window at that altar.
#[derive(Message, Clone, Copy, Debug)]
pub struct AltarOpen(pub Entity);

/// How far the open altar window stays open (`AltarWindow::CloseIfFar`: the window hides itself once
/// the player is farther from its altar than the altar's use distance): the use range at the walk's end
/// (`player_orders::usable_range`).
pub fn altar_reach(db: &gd_formats::vfs::Database, a: &Altar, player_radius: f32) -> f32 {
    let usable = gd_game_core::combat::fixed_usable_range(db.get(&a.record));
    gd_game_core::player_orders::usable_range(player_radius, a.radius, usable, true)
}

/// Within reach of the interacted altar (`player_orders::InteractTarget`): stop and use it (module doc).
#[allow(clippy::too_many_arguments)]
pub fn use_altar(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    #[cfg(feature = "dev")] mut scratch: Local<gd_sim::nav::PathScratch>,
    altar: Res<AltarState>,
    altars: Query<(Entity, &GlobalTransform, &Altar)>,
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
            &Combatant,
        ),
        With<Player>,
    >,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    #[cfg(feature = "dev")] nav: Res<Nav>,
    mut open: MessageWriter<AltarOpen>,
    #[cfg(feature = "dev")] mut tested: Local<bool>,
) {
    let Ok((ptf, mut path, mut interact, pcb)) = players.single_mut() else {
        return;
    };
    // `GD_USE_ALTAR` (hooks.rs): use the nearest altar within 40 m once.
    #[cfg(feature = "dev")]
    if hooks.gameplay.use_altar && !*tested && interact.0.is_none() {
        let near = altars
            .iter()
            .map(|(e, gt, a)| {
                (
                    e,
                    gt.translation(),
                    a.radius,
                    gt.translation().distance(ptf.translation),
                )
            })
            .filter(|x| x.3 < 40.0)
            .min_by(|a, b| a.3.total_cmp(&b.3));
        if let Some((e, at, r, _)) = near {
            *tested = true;
            interact.0 = Some(e);
            if let Some(p) = crate::chests::path_near(&nav.0, &mut scratch, ptf.translation, at, r)
            {
                path.points = p;
            }
        }
    }
    let Some(e) = interact.0 else { return };
    let Ok((_, gt, a)) = altars.get(e) else {
        return;
    };
    let usable = gd_game_core::combat::fixed_usable_range(data.db.get(&a.record));
    let range = gd_game_core::player_orders::usable_range(
        pcb.radius,
        a.radius,
        usable,
        path.points.is_empty(),
    );
    if ptf.translation.distance(gt.translation()) > range {
        return;
    }
    interact.0 = None;
    path.points.clear();
    #[cfg(feature = "dev")]
    let offer = hooks.items.altar_offer.is_some();
    #[cfg(not(feature = "dev"))]
    let offer = false;
    // `GD_ALTAR_OFFER` (hooks.rs) allows it.
    if !(offer || may_use_altar(&data.db, rt.as_deref())) {
        return;
    }
    if altar.activated {
        info!("ascendant: the altar is already active");
        return;
    }
    open.write(AltarOpen(e));
}

/// `MainPlayerCanUseAscendantAltar`: the `ascendantToken` (no token in the game engine
/// record, or the main player holds it).
pub fn may_use_altar(
    db: &gd_formats::vfs::Database,
    rt: Option<&gd_game_core::story::StoryRuntime>,
) -> bool {
    let token = db
        .get("records/game/gameengine.dbr")
        .and_then(|r| r.str("ascendantToken").map(str::to_owned))
        .unwrap_or_default();
    let allowed =
        token.is_empty() || rt.is_some_and(|rt| rt.story.borrow().player.tokens.contains(&token));
    if !allowed {
        info!("ascendant: the altar needs {token}");
    }
    allowed
}

/// The installed formulas reach monster profiles (their attribute paks,
/// `CombatRules`) and Lua (`Game.GetAscendantTokenModifier`).
///
/// `Monster::ContributeGameBalance*Attributes`: a monster of
/// the Shattered Realm's dungeon gets no Ascendant bonus; the ruleset's `difficultyAdjustment*` pak (a
/// `GameAdjustment`, by game difficulty) is read in its place at the index `dungeonsGenerated` (the tier). PORT: it
/// replaces the Ascendant paks for every monster while a dungeon exists (outside it nothing fights).
/// `CharAttributeVal::AddToAccumulator`: the index is 1-based (`gd_sim::ascendant::adjustment_column`:
/// tier 0 adds nothing), and one past the table's end takes its last row (`stats::level_value`).
pub fn apply_altar(
    // The mode (the dungeon ruleset's slot) and the altar's formulas.
    session: crate::session::Session,
    // Option: the story half still runs without rules.
    rules: Option<ResMut<gd_game_core::rules::CombatRules>>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    mut profiles: ResMut<crate::monsters::ProfileCache>,
    // (dungeon stamp, ruleset slot) -> the adjustment pak of its ruleset.
    mut pak: Local<Option<((u32, u32), Option<String>)>>,
    mut fresh: gd_game_core::session::NewSession,
) {
    let state = &session.altar;
    if fresh.begun() {
        *pak = None;
    }
    let dungeon = rt.as_ref().and_then(|rt| {
        let s = rt.story.borrow();
        let d = s.endless.dungeon.as_ref()?;
        let slot = gd_story::endless::Ruleset::slot(&session.mode());
        let key = (d.id, slot as u32);
        if pak.as_ref().is_none_or(|(k, _)| *k != key) {
            let name = gd_story::endless::Ruleset::load(&data.db, &d.ruleset, slot)
                .and_then(|r| r.difficulty_adjustment);
            *pak = Some((key, name));
        }
        let name = pak.as_ref()?.1.clone()?;
        // Tier 0: the pak adds nothing, and the Ascendant bonus is still not read.
        Some(
            gd_sim::ascendant::adjustment_column(s.endless.generated)
                .map(|col| (name, col, gd_sim::ascendant::Scope::All)),
        )
    });
    if let Some(mut rules) = rules {
        let same = match &dungeon {
            Some(d) => rules.0.ascendant_paks.as_slice() == d.as_slice(),
            None => rules.0.ascendant_paks == state.formulas.monster_paks,
        };
        if !same {
            std::sync::Arc::make_mut(&mut rules.0).ascendant_paks = match dungeon {
                Some(d) => d.into_iter().collect(),
                None => state.formulas.monster_paks.clone(),
            };
            profiles.0.clear();
        }
    }
    if let Some(rt) = rt {
        let mut story = rt.story.borrow_mut();
        if story.game.ascendant_tokens != state.formulas.tokens {
            story.game.ascendant_tokens = state.formulas.tokens.clone();
        }
    }
}

/// The ascendant altar: `Use` uses the clicked altar ([`use_altar`]), `Altar` applies its offerings.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AscendantStage {
    Use,
    Altar,
}
