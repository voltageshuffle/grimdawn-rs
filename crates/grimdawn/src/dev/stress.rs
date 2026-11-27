//! `GD_STRESS=<n>`: a sustained load scene in the real world — `n` foes kept fighting around the player,
//! batch kills every few seconds (deaths, gibs, loot, death FX) with the fallen respawned, impact effects
//! in bursts, and a riftgate tour so levels stream in and out (asset load / unload churn).
//!
//! Combine with `GD_SAVE=<player.gdc>` (Devil's Crossing), `GD_GOD`, `GD_AUTO_FIGHT=40`, `GD_FPS=1`,
//! `GD_GPU_TIMES=1` and `GD_SHOT_SECS` to end the run. Not for `GD_TESTWORLD`: the arena has no streaming.
use bevy::prelude::*;
use gd_game_core::hooks::Hooks;
use gd_game_core::monsters::Monster;
use gd_ui::state::session::AppState;

/// The foes, cycled per spawn: melee undead, a fast beast, a ranged aetherial, a human with skills.
const FOES: [&str; 4] = [
    "records/creatures/enemies/zombie_a01.dbr",
    "records/creatures/enemies/beast/barghest_a01.dbr",
    "records/creatures/enemies/aetherial/aetherial_01.dbr",
    "records/creatures/enemies/human/bandit_a01.dbr",
];

/// Impact effects the bursts cycle through (raw `.pfx`, no record indirection).
const BURSTS: [&str; 3] = [
    "particlesystems/combat/pfx_bloodexplosion_01.pfx",
    "particlesystems/combat/pfx_bloodexplosiongiant_02.pfx",
    "particlesystems/combat/pfx_aetherialbloodexplosiongiant_03.pfx",
];

/// Seconds between kill batches, FX bursts and travels.
const KILL_EVERY: f32 = 4.0;
const FX_EVERY: f32 = 0.5;
const TRAVEL_EVERY: f32 = 20.0;
/// FX per burst, scattered within this radius of the player.
const FX_PER_BURST: u32 = 12;
const FX_RADIUS: f32 = 12.0;
/// The foes ring: inner radius and step per ring of 24.
const RING_BASE: f32 = 4.0;
const RING_STEP: f32 = 2.5;
const PER_RING: u32 = 24;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (keep_foes, kill_batch, fx_bursts, travel)
            .chain()
            .run_if(in_state(AppState::InGame)),
    );
}

/// A deterministic scatter (no RNG resource): the golden-angle spiral over the ring index.
fn ring_pos(base: Vec3, i: u32) -> Vec3 {
    let ring = i / PER_RING;
    let k = i % PER_RING;
    let a = k as f32 * 2.399_963 + ring as f32 * 0.7;
    let r = RING_BASE + ring as f32 * RING_STEP;
    base + Vec3::new(a.cos() * r, 0.0, a.sin() * r)
}

#[derive(Default)]
struct Foes {
    /// Spawn serial: the next ring slot and record cycle.
    next: u32,
}

/// Keeps `n` live stress foes around the player: spawns the missing ones once the nav mesh covers their
/// spot (story slot: never culled). The player's own kills and the batch kills both feed this.
#[allow(clippy::too_many_arguments)]
fn keep_foes(
    hooks: Res<Hooks>,
    mut commands: Commands,
    player: Query<&Transform, With<gd_game_core::player::Player>>,
    foes: Query<
        (),
        (
            With<Monster>,
            With<StressFoe>,
            Without<gd_game_core::combat::Dead>,
        ),
    >,
    nav: Res<gd_game::nav::Nav>,
    mut assets: gd_render::character::CharAssets,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    factions: Option<Res<gd_game::factions::PlayerFactions>>,
    mut profiles: ResMut<gd_game::monsters::ProfileCache>,
    mut local: Local<Foes>,
) {
    let (Some(n), Some(rules), Ok(ptf), Some(_)) = (
        hooks.gameplay.stress,
        rules,
        player.single(),
        progress.as_ref(),
    ) else {
        return;
    };
    let missing = n.saturating_sub(foes.iter().len() as u32);
    if missing == 0 {
        return;
    }
    let level = gd_game::monsters::current_player_level(progress.as_deref(), loaded.as_deref());
    let base = ptf.translation;
    // A bounded spawn budget per frame: the bodies / profiles warm up over a few frames instead of one.
    for _ in 0..missing.min(24) {
        let i = local.next;
        let p = ring_pos(base, i);
        let Some(y) = nav.0.ground([p.x, base.y + 50.0, p.z]) else {
            // Off the nav mesh (a wall, the map's edge): try the next slot next frame.
            local.next += 1;
            continue;
        };
        let m = gd_game::monsters::MonsterState {
            record: FOES[(i as usize) % FOES.len()].to_owned(),
            level,
            position: Vec3::new(p.x, y, p.z),
            yaw: 0.0,
            variant: i % 3,
            alive: true,
            pending: false,
            scale: None,
        };
        if let Some(e) = gd_game::monsters::spawn_state(
            &mut commands,
            &assets.data,
            &mut assets.bodies,
            &rules,
            factions.as_deref(),
            &mut profiles,
            &m,
            (usize::MAX, 0, 9000 + i as usize),
            &[],
        ) {
            commands.entity(e).insert(StressFoe);
        }
        local.next += 1;
    }
}

/// Marks a stress scene's foe (the batch kills target only these, never the level's own monsters).
#[derive(Component)]
struct StressFoe;

/// Every [`KILL_EVERY`]: kills a third of the stress foes through the real death path (gibs, loot, death
/// FX, kill messages); `keep_foes` respawns them. Only the stress foes — never the level's own monsters.
fn kill_batch(
    hooks: Res<Hooks>,
    time: Res<Time<Real>>,
    mut since: Local<f32>,
    mut commands: Commands,
    mut foes: Query<
        (
            Entity,
            &Transform,
            &mut gd_game_core::character::Character,
            &mut gd_game_core::combat::Combatant,
            &Monster,
        ),
        (With<StressFoe>, Without<gd_game_core::combat::Dead>),
    >,
    mut states: ResMut<gd_game::monsters::ProxyStates>,
    mut killed: MessageWriter<gd_game::combat::MonsterKilled>,
) {
    if hooks.gameplay.stress.is_none() {
        return;
    }
    *since += time.delta_secs();
    if *since < KILL_EVERY {
        return;
    }
    *since = 0.0;
    for (i, (e, tf, mut ch, mut cb, m)) in foes.iter_mut().enumerate() {
        if i % 3 != 0 {
            continue;
        }
        cb.life = 0.0;
        commands.entity(e).insert(gd_game::survival::ScriptKilled);
        commands.entity(e).insert(gd_game::combat::Overkill);
        gd_game::combat::kill(
            &mut commands,
            e,
            tf.translation,
            Some(&mut ch),
            Some(m),
            false,
            &mut states,
            &mut killed,
            None,
        );
    }
}

/// Every [`FX_EVERY`]: a burst of impact effects around the player, each self-despawning when done.
fn fx_bursts(
    hooks: Res<Hooks>,
    time: Res<Time<Real>>,
    mut commands: Commands,
    player: Query<&Transform, With<gd_game_core::player::Player>>,
    nav: Res<gd_game::nav::Nav>,
    mut since: Local<f32>,
    mut serial: Local<u32>,
) {
    if hooks.gameplay.stress.is_none() {
        return;
    }
    *since += time.delta_secs();
    if *since < FX_EVERY {
        return;
    }
    *since = 0.0;
    let Ok(ptf) = player.single() else { return };
    let base = ptf.translation;
    for _ in 0..FX_PER_BURST {
        let i = *serial;
        *serial = serial.wrapping_add(1);
        let a = i as f32 * 2.399_963;
        let r = FX_RADIUS * ((i % 7) as f32 + 1.0) / 7.0;
        let x = base.x + a.cos() * r;
        let z = base.z + a.sin() * r;
        let y = nav.0.ground([x, base.y + 50.0, z]).unwrap_or(base.y);
        let mut spec = gd_game_core::visuals::EffectSpec::pfx(BURSTS[(i as usize) % BURSTS.len()]);
        spec.despawn_when_complete = true;
        spec.no_loop = true;
        commands.spawn((
            spec,
            Transform::from_xyz(x, y + 0.5, z),
            Visibility::default(),
        ));
    }
}

/// Every [`TRAVEL_EVERY`]: travels to the next riftgate of the map (the memory tour's route), so levels
/// stream in and out and their assets load / unload under the fighting load.
fn travel(
    hooks: Res<Hooks>,
    time: Res<Time<Real>>,
    index: Option<Res<gd_game::riftgates::WaypointIndex>>,
    pending: Res<gd_game::teleport::PendingTravel>,
    mut out: MessageWriter<gd_game::riftgates::GateTravel>,
    mut local: Local<(Vec<Vec3>, usize, Option<f32>)>,
) {
    if hooks.gameplay.stress.is_none() {
        return;
    }
    let Some(index) = index else { return };
    if local.0.is_empty() {
        let mut gates: Vec<_> = index
            .0
            .iter()
            .filter(|(_, p)| p.record.contains("riftgate"))
            .map(|(uid, p)| (*uid, p.pos))
            .collect();
        gates.sort_by_key(|g| g.0);
        local.0 = gates.into_iter().map(|(_, p)| p).collect();
        if local.0.is_empty() {
            return;
        }
    }
    let now = time.elapsed_secs();
    if pending.0.is_some() {
        local.2 = None;
        return;
    }
    let since = *local.2.get_or_insert(now);
    if now - since < TRAVEL_EVERY {
        return;
    }
    let to = local.0[local.1 % local.0.len()];
    info!("stress: travel {} to {to}", local.1 + 1);
    out.write(gd_game::riftgates::GateTravel(to));
    local.1 += 1;
    local.2 = None;
}
