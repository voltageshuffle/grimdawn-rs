//! Monsters as gameplay: who they are (record, level, proxy slot), their
//! faction pack, worn gear, what each proxy spawned this session, and what a
//! dying monster leaves ([`death_actors`], `Skill_OnDeathSpawnActor`).
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_game_core::monsters::{FactionPack, Monster};
use gd_rules::Rng;

/// Items a monster wears (`Character::LoadLoot`, rolled when it is created):
/// they count in its combat profile and drop on death unless Broken.
#[derive(Component)]
pub struct Worn {
    pub items: Vec<gd_formats::save::item::Item>,
    pub resolved: Vec<gd_sim::items::ResolvedItem>,
}

/// The `charLevel` of a character placed straight in a level file (a `Turret`, a town guard),
/// `Character::Load`: its experience level control (`Character.levelControl`) loads the record's `charLevel`
/// equation, `minLevel` (default 1) and `maxLevel` (default 1) (`ExperienceLevelControl::Load`), then
/// `ExperienceLevelControl::ApplyLevel` sets the level (`Character.level`, the constructor leaves 0) to
/// `min(max(trunc(charLevel), minLevel), maxLevel)`, or `minLevel` without an equation. The equation's `charLevel`
/// variable (`ExperienceLevelControl::GetCharLevelVariable`) is the players' average level
/// (`PlayerManagerClient::GetPlayerInfo`: the rounded mean of the players' `Character.level`; one player: his level).
/// The level
/// entity factory sets no level after it, and `Monster::RestoreState` keeps it when
/// the region reloads (here: [`ProxyStates`]). A proxy's monsters get the proxy's level instead.
/// PORT: the equation's `playerLevel` variable (`ExperienceLevelControl.playerLevel`) resolves to 0.
pub fn placed_char_level(db: &gd_formats::vfs::Database, record: &str, player_level: i32) -> i32 {
    let Some(r) = db.get(record) else {
        return 1;
    };
    let min = r.i32("minLevel").unwrap_or(1).max(0);
    let max = r.i32("maxLevel").unwrap_or(1).max(0);
    let Some(eq) = r
        .str("charLevel")
        .filter(|e| !e.is_empty())
        .and_then(|e| gd_rules::expr::Expr::parse(e).ok())
    else {
        return min;
    };
    let v = eq.eval(&|n| {
        n.eq_ignore_ascii_case("charLevel")
            .then_some(player_level as f32)
    }) as i32;
    v.max(min).min(max)
}

/// A placed NPC of a level (despawned with it; `usize::MAX`: a story monster's, left to the story).
#[derive(Component)]
pub struct LevelNpc(pub usize);

#[derive(Clone, Debug)]
pub struct MonsterState {
    pub record: String,
    pub level: i32,
    pub position: Vec3,
    pub yaw: f32,
    pub variant: u32,
    pub alive: bool,
    /// Held back by an ambush proxy until it releases it.
    pub pending: bool,
    /// A level placement's scale (`creature` module doc); None: the record's.
    pub scale: Option<f32>,
}

/// The pool each "Unified Entities" group chose this session (group name -> index into its members'
/// candidate pools; `GameEngine.unifiedEntityChoices`, cleared in `ExitPlayingMode`): its proxies and set pieces
/// share
/// the choice.
#[derive(Resource, Default)]
pub struct UnifiedPools(pub HashMap<String, usize>);

/// What each proxy (level, entity index) spawned this session.
#[derive(Resource, Default)]
pub struct ProxyStates(pub HashMap<(usize, u32), Vec<MonsterState>>);

/// The session's hero-boss registry (`GameEngine::RegisterProxyHeroBoss` / `GameEngine::IsProxyHeroBossInGame`,
/// Game.dll): the lowercased record paths of the Hero / Boss / SuperBoss
/// monsters the proxies spawned. A pool entry whose record is in it never spawns again this session
/// (`gd_sim::proxy::run_pool`); a proxy that ignores the restriction (`Proxy.Create`'s bool,
/// `ProxyEndless`) spawns its bosses without registering them. Nothing unregisters
/// (`UnregisterProxyHeroBoss` has no callers): the registry lasts the session (reset at its end).
#[derive(Resource, Default)]
pub struct HeroBosses(pub gd_formats::collections::HashSet<String>);

/// Monster profiles by (record, level).
#[derive(Resource, Default)]
pub struct ProfileCache(
    pub HashMap<(String, i32), Option<(std::sync::Arc<gd_sim::actor::Profile>, f32)>>,
);

pub const PLAYER_PACK: &str = "records/controllers/factions/faction_player.dbr";

/// Whether the player treats `record` as a foe (`FactionManager::
/// IsFoe` (both overloads) with the player's pack): the
/// creature's faction is its `factions` pack's `myFaction` (none, e.g. no
/// pack: never a foe); the player's own value for that faction decides when
/// it is non-zero (foe below 0), else the creature pack's value for the
/// player's faction (`factionPlayer`) does. `defaultTeamMajor` is not read by
/// the game. The player's values are the session's (`factions`), before
/// loading the player record's pack (`faction_player.dbr`).
pub fn is_foe(
    db: &gd_formats::vfs::Database,
    factions: Option<&crate::factions::PlayerFactions>,
    record: &str,
) -> bool {
    let Some(pack) = db.get(record).and_then(|r| {
        r.str("factions")
            .filter(|p| !p.is_empty())
            .and_then(|p| db.get(p))
    }) else {
        return false;
    };
    let Some(t) = pack
        .str("myFaction")
        .and_then(gd_game_core::factions::index_of_name)
    else {
        return false;
    };
    let mine = player_value(db, factions, t);
    if mine != 0.0 {
        return mine < 0.0;
    }
    pack.f32("factionPlayer").unwrap_or(0.0) < 0.0
}

/// The player's value for faction `t` ([`is_foe`]): the session's, else the player record's pack.
pub fn player_value(
    db: &gd_formats::vfs::Database,
    factions: Option<&crate::factions::PlayerFactions>,
    t: usize,
) -> f32 {
    let saved = factions.and_then(|f| f.list.get(t).map(|x| x.value));
    saved.unwrap_or_else(|| {
        let key = gd_rules::faction::KEYS.get(t).copied().unwrap_or_default();
        db.get(PLAYER_PACK).and_then(|p| p.f32(key)).unwrap_or(0.0)
    })
}

/// The player's faction pack as `Monster::JoinMe` copies it onto a monster joining him (the
/// joiner's pack `Character.factionPack` becomes the leader's): the player record pack's `myFaction`
/// (`faction_player.dbr`, the
/// player faction 0) and the player's values ([`player_value`]).
pub fn player_pack(
    db: &gd_formats::vfs::Database,
    factions: Option<&crate::factions::PlayerFactions>,
) -> FactionPack {
    FactionPack {
        mine: db
            .get(PLAYER_PACK)
            .and_then(|p| {
                p.str("myFaction")
                    .and_then(gd_game_core::factions::index_of_name)
            })
            .unwrap_or(0),
        values: (0..gd_rules::faction::FACTION_COUNT)
            .map(|t| player_value(db, factions, t))
            .collect(),
    }
}

/// Creatures that are not the player's foes ([`is_foe`]): they stand as
/// plain NPCs and cannot be attacked, unless they fight ([`is_fighter`]).
pub fn is_friendly(
    db: &gd_formats::vfs::Database,
    factions: Option<&crate::factions::PlayerFactions>,
    record: &str,
) -> bool {
    !is_foe(db, factions, record)
}

/// Whether a creature record is a fighting monster: class `Monster` with a `controller`. A friendly one is
/// `combat::Allied` and fights its faction foes, however it got into the world: a Crucible defense or
/// Devil's Crossing guard spawned by the story, a town guard placed straight in a level
/// ([`LevelProxies::npcs`]), a friendly creature a proxy rolled. The
/// engine has no ally flag nor team table: every `Monster` runs its `ControllerMonster`, whose
/// `AngerUpdate` takes as targets the characters in its view sphere that `FactionManager::IsFoe`
/// calls foes, so a placed guard (`factions` Survivors, a `ControllerMonster` /
/// `ControllerStationaryMonster`) engages the monsters its faction pack is hostile to, like any monster engages
/// the player. Other friendly records (`Npc*` classes, or no controller) are plain NPCs.
pub fn is_fighter(db: &gd_formats::vfs::Database, record: &str) -> bool {
    db.get(record)
        .is_some_and(|r| r.class == "Monster" && r.str("controller").is_some_and(|c| !c.is_empty()))
}

/// Whether a creature record is of the engine's `Npc` class family (`Npc`, `NpcMerchant`, `NpcCrafter`, ...:
/// every `Npc*` creature class, by their RTTI parents in Game.dll). Such a character never fights nor
/// takes a skill hit, whatever its faction: `Npc::Load` gives it a `ControllerNpc2` (default
/// `ai_npcdefault02`: wander / idle / chat / quest states, no anger nor attack state) and
/// `GameEngine::FilterInvalidTargets` drops every `Npc`. So a Kurn or Bysmiel NPC stays a plain NPC
/// when the player's standing with its faction is hostile, and a reputation change never turns it.
pub fn is_npc(db: &gd_formats::vfs::Database, record: &str) -> bool {
    db.get(record).is_some_and(|r| r.class.starts_with("Npc"))
}

/// Whether a spawned creature stands as a plain NPC (no `Monster`, no AI): an [`is_npc`] record, or a friendly
/// ([`is_friendly`]) one that does not fight ([`is_fighter`]).
pub fn stands_as_npc(
    db: &gd_formats::vfs::Database,
    factions: Option<&crate::factions::PlayerFactions>,
    record: &str,
) -> bool {
    is_npc(db, record) || (is_friendly(db, factions, record) && !is_fighter(db, record))
}

/// Spawns one proxy monster with its combat profile and AI (friendly
/// creatures, [`is_friendly`] by the player's `factions`, are `combat::Allied` when they fight
/// ([`is_fighter`]), else plain NPCs).
#[allow(clippy::too_many_arguments)]
pub fn spawn_state(
    commands: &mut Commands,
    data: &gd_game_core::data::GameData,
    bodies: &mut gd_game_core::creature::BodyCache,
    rules: &gd_game_core::rules::CombatRules,
    factions: Option<&crate::factions::PlayerFactions>,
    profiles: &mut ProfileCache,
    m: &MonsterState,
    slot: (usize, u32, usize),
    patrol: &[[u32; 4]],
) -> Option<Entity> {
    let tf = Transform::from_translation(m.position).with_rotation(Quat::from_rotation_y(m.yaw));
    let e =
        crate::creature::spawn_creature(commands, data, bodies, tf, &m.record, m.variant, m.scale)?
            .0;
    let prof = profiles
        .0
        .entry((m.record.clone(), m.level))
        .or_insert_with(|| {
            let p = gd_sim::actor::monster(
                &data.db,
                &rules.0,
                &m.record,
                m.level,
                gd_sim::actor::Side::Monster,
            )?;
            let radius = gd_game_core::combat::get_radius(data.db.get(&m.record), 0.5);
            Some((std::sync::Arc::new(p), radius))
        })
        .clone();
    if stands_as_npc(&data.db, factions, &m.record) {
        return Some(e);
    }
    if is_friendly(&data.db, factions, &m.record) {
        commands.entity(e).insert(crate::combat::Allied);
    }
    commands.entity(e).insert(Monster {
        record: m.record.clone(),
        level: m.level,
        slot,
    });
    if let Some((p, radius)) = prof {
        let mut ai = crate::combat::MonsterAi::from_record(data, &m.record, m.position);
        ai.patrol.replace(patrol.to_vec());
        commands
            .entity(e)
            .insert((gd_game_core::combat::Combatant::new(p, radius), ai));
    }
    Some(e)
}

/// A `Proxy` entity of a level (collected by the world streamer).
#[derive(Clone, Debug)]
pub struct ProxyCpu {
    /// Entity index in the level.
    pub entity: u32,
    pub record: String,
    pub position: Vec3,
    /// The level entity's uid (its instance-group membership).
    pub uid: Option<[u32; 4]>,
}

/// On a level root: its proxies, not yet run/restored.
#[derive(Component)]
pub struct LevelProxies {
    pub level: usize,
    pub proxies: Vec<ProxyCpu>,
    /// NPCs and `Monster`s (town guards) without story hooks: (entity index, record, transform); the transform's
    /// scale is the placement's (`creature` module doc).
    pub npcs: Vec<(u32, String, Transform)>,
    /// Loot containers without story hooks: (entity index, record, transform).
    pub chests: Vec<(u32, String, Transform)>,
    /// Set pieces: (entity index, record, position, yaw, uid).
    pub set_pieces: Vec<(u32, String, Vec3, f32, Option<[u32; 4]>)>,
    /// Doors and dungeon entrances without story hooks: (record, transform, uid).
    pub doors: Vec<(String, Transform, Option<[u32; 4]>)>,
    /// Breakable objects: (entity index, record, transform).
    pub breakables: Vec<(u32, String, Transform)>,
    /// `NavBlocker`s without story hooks: (record, position).
    pub blockers: Vec<(String, Vec3)>,
    /// Riftgates and respawn points: (record, position, uid).
    pub waypoints: Vec<(String, Vec3, [u32; 4])>,
    pub shrines: Vec<(String, Transform, [u32; 4])>,
    pub sounds: Vec<(String, Vec3)>,
    /// `PatrolPoint` objects: (record, position, uid).
    pub patrol_points: Vec<(String, Vec3, [u32; 4])>,
    /// Gameplay objects placed straight in the level, by record class
    /// ([`placed_class`]): (entity index, record, transform).
    pub placed: Vec<(u32, String, Transform)>,
}

/// Whether a level entity of record class `class` is one of the
/// [`LevelProxies::placed`] objects: `CharonGeyserMarker` (`crate::geysers`)
/// and `Turret` monsters (`crate::turrets`).
pub fn placed_class(class: &str) -> bool {
    matches!(class, "CharonGeyserMarker" | "Turret")
}

/// An entity of a level outside its root, despawned with the level ([`cull_monsters`]): a static object a set piece
/// placed, and the loose world entities that take the level they appear in (`crate::levels::place_loose`; ground
/// items are saved first, `crate::loot::LevelItems`).
#[derive(Component)]
pub struct LevelProp(pub usize);

/// An object a set piece placed this session.
#[derive(Clone, Debug)]
pub struct SetObject {
    pub record: String,
    pub class: String,
    pub position: Vec3,
    /// Heading of `rot` (for creatures).
    pub yaw: f32,
    pub rot: Quat,
    pub scale: f32,
    /// `blockPathingN`: blocks the nav mesh with its hit boxes.
    pub block: bool,
}

/// What each set piece (level, entity index) placed this session.
#[derive(Resource, Default)]
pub struct SetPieceStates(pub HashMap<(usize, u32), Vec<SetObject>>);

/// The engine's "request and block" points while one set piece is placed
/// (Engine `NavManager::RequestBlockedPoint` & co: each accepted point is kept with its
/// clearance; a point is blocked when its xz distance to a kept one is at most
/// the two clearances; `SetPiece::Place` clears them afterwards).
struct Blocks<'a> {
    nav: &'a gd_sim::nav::NavWorld,
    y: f32,
    kept: Vec<(Vec3, f32)>,
}

impl Blocks<'_> {
    fn blocked(&self, p: Vec3, r: f32) -> bool {
        self.kept
            .iter()
            .any(|(q, qr)| (p - *q).with_y(0.0).length() <= r + qr)
    }

    /// The navmesh point under (x, z) (Detour's nearest point must be the
    /// candidate itself).
    fn on_mesh(&self, x: f32, z: f32) -> Option<Vec3> {
        self.nav
            .ground([x, self.y + 2.0, z])
            .map(|g| Vec3::new(x, g, z))
    }

    fn take(&mut self, p: Vec3, r: f32) -> Option<Vec3> {
        if self.blocked(p, r) {
            return None;
        }
        self.kept.push((p, r));
        Some(p)
    }

    /// `RequestAndBlockRandomPointInRadius`: 100 tries at a random angle and
    /// a random distance up to `radius`.
    fn in_radius(&mut self, c: Vec3, radius: f32, r: f32) -> Option<Vec3> {
        for _ in 0..100 {
            let (a, d) = (
                gd_game_core::rng::f32() * std::f32::consts::TAU,
                gd_game_core::rng::f32() * radius,
            );
            if let Some(p) = self
                .on_mesh(c.x + a.cos() * d, c.z + a.sin() * d)
                .filter(|p| !self.blocked(*p, r))
            {
                return self.take(p, r);
            }
        }
        None
    }

    /// `RequestAndBlockRandomPointOnRadius`: 100 tries on the circle.
    fn on_radius(&mut self, c: Vec3, radius: f32, r: f32) -> Option<Vec3> {
        for _ in 0..100 {
            let a = gd_game_core::rng::f32() * std::f32::consts::TAU;
            if let Some(p) = self
                .on_mesh(c.x + a.cos() * radius, c.z + a.sin() * radius)
                .filter(|p| !self.blocked(*p, r))
            {
                return self.take(p, r);
            }
        }
        None
    }

    /// `RequestAndBlockNearPoint`: rings 2r + 0.1 apart out to `max`, as many
    /// samples per ring as fit, from a random start angle that keeps turning.
    fn near(&mut self, c: Vec3, max: f32, r: f32) -> Option<Vec3> {
        if max <= 0.0 {
            return None;
        }
        let step = 2.0 * r + 0.1;
        let (mut ring, mut n, mut da) = (0.0f32, 1i32, 0.0f32);
        let mut a = gd_game_core::rng::f32() * std::f32::consts::TAU;
        loop {
            for _ in 0..n {
                if let Some(p) = self
                    .on_mesh(c.x + a.sin() * ring, c.z + a.cos() * ring)
                    .filter(|p| !self.blocked(*p, r))
                {
                    return self.take(p, r);
                }
                a += da;
            }
            ring += step;
            n = (ring * std::f32::consts::TAU / step) as i32;
            da = std::f32::consts::TAU / n.max(1) as f32;
            if ring >= max {
                return None;
            }
        }
    }
}

/// An ambush proxy holding back monsters (`ProxyAmbush::UpdateSelf`):
/// once the player is inside `alertArea`, after
/// `rand[minDelayTime, maxDelayTime]` it places `rand[minGroupSize,
/// maxGroupSize]` of them every `rand[minSpawnTime, maxSpawnTime]` (ms),
/// waiting `minSpawnTime` while more than `spawnThreshold` of its monsters
/// are alive. Times are the record's seconds × 1000 (`Load`). Each placed
/// monster gets its Spawn animation (`PlaceNextObject`, [`crate::combat::SpawnStartup`]).
#[derive(Component)]
pub struct Ambush {
    key: (usize, u32),
    position: Vec3,
    alert: f32,
    spawn_ms: (i32, i32),
    delay_ms: (i32, i32),
    group: (i32, i32),
    threshold: usize,
    alerted: bool,
    timer: i32,
}

/// A Shattered Realm proxy (`ProxyEndless`): its monsters are held (`pending`) in [`ProxyStates`] until the
/// generator activates the proxy ([`crate::endless`], `ProxyEndless::UpdateSelf`: state
/// 1 = dormant never spawns, the generator flips it to 0 when the floor is active). `LevelProp` of its level.
#[derive(Component)]
pub struct EndlessProxy {
    pub key: (usize, u32),
    /// The proxy record (a shrine proxy's accessory pools: `crate::endless::spawn_activated_shrines`).
    pub record: String,
    pub uid: Option<[u32; 4]>,
    /// The patrol points the monsters take on activation.
    pub patrol: Vec<[u32; 4]>,
}

/// Key of the n-th object of a level's set piece `ei` (proxy/chest keys of
/// its objects; level entity indices stay below 2^24).
pub fn set_key(ei: u32, n: usize) -> u32 {
    0x8000_0000 | (ei << 6) | n as u32 & 63
}

/// Positions for a set piece's objects (`SetPiece::Place`
/// and its `Apply*Positioning`): parts stand at their offset
/// from the set piece (world axes, put on the floor when the offset has no
/// height) and block their spot; Random puts each object at a free point
/// within `placementRadius` (`GetFreeLocation`), Ring on the circle
/// (`GetOutlyingLocation`); both first roll the scale (own stream). Camp
/// places the parts, then one random main/center object near the centre
/// (`GetNearbyLocation`) and the other main ones and all secondary /
/// perimeter ones on the circle, then the rest: the first object of each
/// entry at a free point, the later ones near the last placed one of their
/// entry (which moves on while inside the radius). Grid puts each object on
/// a random free cell (2 × the mean radius, at least 2 m) of the disc,
/// nudged along x by up to half a cell. Clearance is the object's radius
/// (`actorRadius` × the scale vector's length) + its `buffer`; a forced
/// object that finds no room takes the set piece's position, any other is
/// dropped. Then `ApplyRotation` (own stream).
pub fn place_set_piece(
    nav: &gd_sim::nav::NavWorld,
    centre: Vec3,
    yaw: f32,
    run: &gd_sim::setpiece::SetPieceRun,
) -> Vec<SetObject> {
    use gd_sim::setpiece::{Scheme, TAG_MAIN, TAG_SECONDARY};
    let mut own = run.stream;
    let mut blocks = Blocks {
        nav,
        y: centre.y,
        kept: Vec::new(),
    };
    let radius = run.radius;
    let n = run.objects.len();
    // (position, scale) per object; None = not placed.
    let mut placed: Vec<Option<(Vec3, f32)>> = vec![None; n];
    let floor = |p: Vec3| {
        nav.ground([p.x, p.y + 2.0, p.z])
            .map_or(p, |g| Vec3::new(p.x, g, p.z))
    };
    let clearance = |o: &gd_sim::setpiece::Placement, s: f32| o.radius * s * 3f32.sqrt() + o.buffer;
    let roll_scale = |own: &mut Rng, o: &gd_sim::setpiece::Placement| {
        if o.scale.1 <= o.scale.0 {
            o.scale.0
        } else {
            o.scale.0 + own.unit() * (o.scale.1 - o.scale.0)
        }
    };
    let part = |blocks: &mut Blocks, o: &gd_sim::setpiece::Placement, s: f32| -> Vec3 {
        let off = o.offset.unwrap_or_default();
        let p = centre + Vec3::from(off);
        let p = if off[1] == 0.0 { floor(p) } else { p };
        blocks.take(p, clearance(o, s));
        p
    };
    let forced =
        |o: &gd_sim::setpiece::Placement, p: Option<Vec3>| p.or(o.force.then_some(floor(centre)));
    match run.scheme {
        Scheme::Random | Scheme::Ring => {
            for (i, o) in run.objects.iter().enumerate() {
                let s = roll_scale(&mut own, o);
                let p = if o.offset.is_some() {
                    Some(part(&mut blocks, o, s))
                } else if run.scheme == Scheme::Ring {
                    forced(o, blocks.on_radius(centre, radius, clearance(o, s)))
                } else {
                    forced(o, blocks.in_radius(centre, radius, clearance(o, s)))
                };
                placed[i] = p.map(|p| (p, s));
            }
        }
        Scheme::Camp => {
            let (mut main, mut second, mut rest) = (Vec::new(), Vec::new(), Vec::new());
            for (i, o) in run.objects.iter().enumerate() {
                if o.offset.is_some() {
                    placed[i] = Some((part(&mut blocks, o, o.scale.0), o.scale.0));
                } else if o.tags & TAG_MAIN != 0 {
                    main.push(i);
                } else if o.tags & TAG_SECONDARY != 0 {
                    second.push(i);
                } else {
                    rest.push(i);
                }
            }
            if !main.is_empty() {
                let k = (own.next() % main.len() as u32) as usize;
                let i = main[k];
                let o = &run.objects[i];
                placed[i] = forced(o, blocks.near(centre, radius, clearance(o, o.scale.0)))
                    .map(|p| (p, o.scale.0));
                let last = main.len() - 1;
                main[k] = main[last];
                main.pop();
            }
            for &i in main.iter().chain(&second) {
                let o = &run.objects[i];
                placed[i] = forced(o, blocks.on_radius(centre, radius, clearance(o, o.scale.0)))
                    .map(|p| (p, o.scale.0));
            }
            let mut anchors: HashMap<usize, Vec3> = HashMap::default();
            for &i in &rest {
                let o = &run.objects[i];
                let c = clearance(o, o.scale.0);
                let p = match (o.entry, anchors.get(&o.entry).copied()) {
                    (0, _) => forced(o, blocks.in_radius(centre, radius, c)),
                    (e, None) => {
                        let p = forced(o, blocks.in_radius(centre, radius, c));
                        if let Some(p) = p {
                            anchors.insert(e, p);
                        }
                        p
                    }
                    (e, Some(a)) => {
                        let p = blocks.near(a, radius, c).or(o.force.then_some(a));
                        if let Some(p) = p
                            && (p - centre).length() < radius
                        {
                            anchors.insert(e, p);
                        }
                        p
                    }
                };
                placed[i] = p.map(|p| (p, o.scale.0));
            }
        }
        Scheme::Grid => {
            let objs: Vec<&gd_sim::setpiece::Placement> =
                run.objects.iter().filter(|o| o.offset.is_none()).collect();
            let mean = if objs.is_empty() {
                0.0
            } else {
                objs.iter()
                    .map(|o| o.radius * o.scale.0 * 3f32.sqrt())
                    .sum::<f32>()
                    / objs.len() as f32
            };
            let cell = (mean * 2.0).max(2.0);
            let mut cells = Vec::new();
            let mut z = -radius;
            while z <= radius {
                let mut x = -radius;
                while x <= radius {
                    if x * x + z * z <= radius * radius {
                        cells.push((x, z));
                    }
                    x += cell;
                }
                z += cell;
            }
            for (i, o) in run.objects.iter().enumerate() {
                if cells.is_empty() {
                    break;
                }
                if o.offset.is_some() {
                    placed[i] = Some((part(&mut blocks, o, o.scale.0), o.scale.0));
                    continue;
                }
                let k = (own.next() % cells.len() as u32) as usize;
                let (mut x, z) = cells.swap_remove(k);
                let a = own.unit() * std::f32::consts::TAU;
                let d = own.unit() * cell * 0.5;
                // The game nudges the cell's x and its (discarded) height.
                x += a.cos() * d;
                let _height = a.sin();
                let p = floor(centre + Vec3::new(x, 0.0, z));
                let p = blocks.take(p, clearance(o, o.scale.0));
                placed[i] = forced(o, p).map(|p| (p, o.scale.0));
            }
        }
    }
    let mut out = Vec::new();
    for (o, at) in run.objects.iter().zip(placed) {
        let Some((p, s)) = at else { continue };
        // `ApplyRotation` (parts keep the set piece's frame).
        let rot = if o.offset.is_some() {
            Quat::from_rotation_y(yaw)
        } else {
            set_rotation(&mut own, o.rotation, p, centre, yaw)
        };
        let fwd = rot * Vec3::Z;
        out.push(SetObject {
            record: o.record.clone(),
            class: String::new(),
            position: p,
            yaw: fwd.x.atan2(fwd.z),
            rot,
            scale: s,
            block: o.block_pathing,
        });
    }
    out
}

/// `SetPiece::ApplyRotation` (forward = the model's +Z): None
/// keeps the set piece's facing; Y a random heading; XYZ a random up vector
/// (three rolls in -1..1); Aligned along the line from the centre, turned a
/// quarter at random (odd roll); FaceOut / FaceIn away from / towards the
/// centre; a point within 0.1 m of the centre gets a random heading.
pub fn set_rotation(
    own: &mut Rng,
    r: gd_sim::setpiece::Rotation,
    p: Vec3,
    centre: Vec3,
    yaw: f32,
) -> Quat {
    use gd_sim::setpiece::Rotation;
    let heading = |f: Vec3| Quat::from_rotation_y(f.x.atan2(f.z));
    let random = |own: &mut Rng| {
        let a = own.unit() * std::f32::consts::TAU;
        heading(Vec3::new(a.cos(), 0.0, a.sin()))
    };
    let out = p - centre;
    match r {
        Rotation::None => Quat::from_rotation_y(yaw),
        Rotation::Y => random(own),
        Rotation::Xyz => {
            let v = Vec3::new(
                own.unit() * 2.0 - 1.0,
                own.unit() * 2.0 - 1.0,
                own.unit() * 2.0 - 1.0,
            );
            let up = if v.length_squared() > 0.1 {
                v.normalize()
            } else {
                Vec3::Y
            };
            Quat::from_rotation_arc(Vec3::Y, up)
        }
        Rotation::Aligned => {
            let base = if out.length() < 0.1 {
                random(own)
            } else {
                heading(out.normalize())
            };
            if own.next() & 1 != 0 {
                base * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
            } else {
                base
            }
        }
        Rotation::FaceOut if out.length() >= 0.1 => heading(out.normalize()),
        Rotation::FaceIn if out.length() >= 0.1 => heading(-out.normalize()),
        Rotation::FaceOut | Rotation::FaceIn => random(own),
    }
}

pub fn irand(lo: i32, hi: i32) -> i32 {
    if hi <= lo {
        lo
    } else {
        gd_game_core::rng::i32(lo..=hi)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn ambushes(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<&mut Ambush>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    monsters: Query<&Monster, Without<gd_game_core::combat::Dead>>,
    mut states: ResMut<ProxyStates>,
    mut profiles: ResMut<ProfileCache>,
    rules: Res<gd_game_core::rules::CombatRules>,
    mut assets: crate::creature::Spawner,
    // Option: factions are optional input to the spawn (`as_deref`).
    factions: Option<Res<crate::factions::PlayerFactions>>,
    // The fraction of a millisecond the whole-millisecond timers have not consumed yet.
    mut carry: Local<f32>,
    (mut anims, mut clips, mut new): (
        ResMut<gd_game_core::character::AnimCache>,
        Local<crate::creature::ClipLists>,
        gd_game_core::session::NewSession,
    ),
) {
    if new.begun() {
        clips.clear();
    }
    let Ok(ptf) = players.single() else {
        return;
    };
    let dt = gd_game_core::session::frame_ms(&mut carry, &time);
    let deadline = std::time::Instant::now() + crate::creature::CLIPS_BUDGET;
    for mut a in &mut q {
        if !a.alerted {
            if ptf.translation.distance(a.position) > a.alert {
                continue;
            }
            a.alerted = true;
            a.timer = irand(a.delay_ms.0, a.delay_ms.1);
            info!("ambush {:?} alerted at {:?}", a.key, a.position);
            // PORT: its group's meshes and clips start loading now, hidden by the spawn delay
            // ([`crate::creature::ready_to_spawn`]).
            if let Some(list) = states.0.get(&a.key) {
                for m in list.iter().filter(|m| m.pending) {
                    crate::creature::ready_to_spawn(
                        &mut assets,
                        &mut anims,
                        &mut clips,
                        &m.record,
                        m.variant,
                        deadline,
                    );
                }
            }
            continue;
        }
        a.timer -= dt;
        if a.timer > 0 {
            continue;
        }
        let key = a.key;
        let alive = monsters
            .iter()
            .filter(|m| (m.slot.0, m.slot.1) == key)
            .count();
        if alive > a.threshold {
            a.timer = a.spawn_ms.0;
            continue;
        }
        let Some(list) = states.0.get_mut(&key) else {
            continue;
        };
        // PORT: a group whose meshes or clips are still loading waits (its timer run out) for a later frame.
        let mut ready = true;
        for m in list.iter().filter(|m| m.pending) {
            ready &= crate::creature::ready_to_spawn(
                &mut assets,
                &mut anims,
                &mut clips,
                &m.record,
                m.variant,
                deadline,
            );
        }
        if !ready {
            continue;
        }
        let mut n = irand(a.group.0, a.group.1);
        for (i, m) in list.iter_mut().enumerate() {
            if n <= 0 {
                break;
            }
            if m.pending {
                m.pending = false;
                let placed = spawn_state(
                    &mut commands,
                    &assets.data,
                    &mut assets.bodies,
                    &rules,
                    factions.as_deref(),
                    &mut profiles,
                    m,
                    (key.0, key.1, i),
                    &[],
                );
                // `PlaceNextObject`: `EnableSpawnAnimation` on a monster before it is
                // spawned (`combat::SpawnStartup`; a hidden monster's Startup is its Hidden state instead).
                if let Some(e) = placed {
                    commands.entity(e).queue_silenced(|mut w: EntityWorldMut| {
                        if let Some(mut ai) = w.get_mut::<crate::combat::MonsterAi>() {
                            ai.enable_spawn_animation();
                        }
                    });
                }
                n -= 1;
            }
        }
        a.timer = irand(a.spawn_ms.0, a.spawn_ms.1);
    }
}

/// A level monster [`run_proxies`] queued: its [`ProxyStates`] entry (key, index in the list) and its patrol points.
struct PendingSpawn {
    key: (usize, u32),
    index: usize,
    patrol: std::sync::Arc<[[u32; 4]]>,
}

/// On a populated level root: its monsters still to spawn ([`spawn_level_monsters`]).
#[derive(Component)]
pub struct PendingMonsters(Vec<PendingSpawn>);

/// Spawns the monsters [`run_proxies`] queued on the level roots ([`PendingMonsters`]), each once its mesh and clips
/// are parsed ([`crate::creature::ready_to_spawn`]), at most [`crate::creature::SPAWNS_PER_FRAME`] a frame. PORT: the
/// engine spawns a level's monsters with the level; here they come over the next frames, so walking into a level
/// does not stall a frame on dozens of spawns. A monster whose state died or turned pending meanwhile is dropped.
#[allow(clippy::too_many_arguments)]
pub fn spawn_level_monsters(
    mut commands: Commands,
    mut levels: Query<&mut PendingMonsters>,
    states: Res<ProxyStates>,
    mut profiles: ResMut<ProfileCache>,
    rules: Res<gd_game_core::rules::CombatRules>,
    mut assets: crate::creature::Spawner,
    factions: Option<Res<crate::factions::PlayerFactions>>,
    mut anims: ResMut<gd_game_core::character::AnimCache>,
    mut clips: Local<crate::creature::ClipLists>,
    mut new: gd_game_core::session::NewSession,
) {
    if new.begun() {
        clips.clear();
    }
    let mut budget = crate::creature::SPAWNS_PER_FRAME;
    let deadline = std::time::Instant::now() + crate::creature::CLIPS_BUDGET;
    for mut queue in &mut levels {
        queue.0.retain(|s| {
            let Some(m) = states.0.get(&s.key).and_then(|l| l.get(s.index)) else {
                return false;
            };
            if !m.alive || m.pending {
                return false;
            }
            let ready = crate::creature::ready_to_spawn(
                &mut assets,
                &mut anims,
                &mut clips,
                &m.record,
                m.variant,
                deadline,
            );
            if !ready || budget == 0 {
                return true;
            }
            budget -= 1;
            spawn_state(
                &mut commands,
                &assets.data,
                &mut assets.bodies,
                &rules,
                factions.as_deref(),
                &mut profiles,
                m,
                (s.key.0, s.key.1, s.index),
                &s.patrol,
            );
            false
        });
    }
}

/// Scatters `n` points on the nav mesh around `center`, at least 1.2 m apart
/// (stand-in for `NavManager::FillPointSet`, which packs 0.25-radius circles).
pub fn place(nav: &gd_sim::nav::NavWorld, center: Vec3, extents: f32, n: usize) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::new();
    let base = nav
        .nearest(center.to_array(), 4.0)
        .map(|c| Vec3::from(nav.pos(c)))
        .unwrap_or(center);
    for i in 0..n {
        let mut chosen = None;
        for attempt in 0..24 {
            let r = extents.max(1.0) * (0.3 + 0.7 * (attempt as f32 / 24.0));
            let p = base
                + Vec3::new(
                    (gd_game_core::rng::f32() * 2.0 - 1.0) * r,
                    2.0,
                    (gd_game_core::rng::f32() * 2.0 - 1.0) * r,
                );
            let Some(c) = nav.nearest(p.to_array(), 1.5) else {
                continue;
            };
            let q = Vec3::from(nav.pos(c));
            if (q.y - base.y).abs() < 4.0 && out.iter().all(|o| o.distance(q) > 1.2) {
                chosen = Some(q);
                break;
            }
        }
        out.push(chosen.unwrap_or(base + Vec3::new(i as f32 * 0.8, 0.0, 0.0)));
    }
    out
}

/// Monsters are not parented to their level (streaming may despawn a level in
/// the frame they are added); they go when their level is unloaded: marked
/// [`gd_game_core::levels::Unload`], despawned in `Last` (other systems of this frame
/// may still command them). Only a change of the loaded levels, or an entity
/// added since the last run (spawned for a level already gone), can need it.
pub fn cull_monsters(
    mut commands: Commands,
    world: Res<gd_game_core::levels::Levels>,
    q: Query<(Entity, Ref<Monster>)>,
    npcs: Query<(Entity, Ref<LevelNpc>)>,
    // Ground items leave with their level through `crate::loot::stash_ground_items` (kept for its next load).
    props: Query<(Entity, Ref<LevelProp>), Without<crate::loot::GroundItem>>,
) {
    let all = world.is_changed();
    let gone = |level: usize| !world.loaded.contains_key(&level);
    for (e, m) in &q {
        // Story monsters (slot level usize::MAX) are managed by the story.
        if (all || m.is_added()) && m.slot.0 != usize::MAX && gone(m.slot.0) {
            commands.entity(e).try_insert(gd_game_core::levels::Unload);
        }
    }
    for (e, n) in &npcs {
        // A story monster's dropped NPC keeps its slot level usize::MAX: the story's too.
        if (all || n.is_added()) && n.0 != usize::MAX && gone(n.0) {
            commands.entity(e).try_insert(gd_game_core::levels::Unload);
        }
    }
    for (e, n) in &props {
        if (all || n.is_added()) && gone(n.0) {
            commands.entity(e).try_insert(gd_game_core::levels::Unload);
        }
    }
}

/// The player's level now, for what spawns: [`gd_game_core::progress::PlayerProgress`] (the one source of the level
/// in a session). Before `init_progress` created it (the menu backdrop, the frames a session's first levels
/// stream in ahead of the player) it is the level that resource starts from (`PlayerProgress::from_save`).
fn current_player_level(
    progress: Option<&gd_game_core::progress::PlayerProgress>,
    loaded: Option<&gd_game_core::session::LoadedCharacter>,
) -> i32 {
    progress
        .map_or_else(
            || {
                gd_game_core::progress::PlayerProgress::from_save(loaded)
                    .progress
                    .level
            },
            |p| p.progress.level,
        )
        .max(1)
}

/// New monsters roll and equip their gear, then rebuild their profile.
#[allow(clippy::too_many_arguments)]
pub fn equip_monsters(
    mut commands: Commands,
    mut q: Query<(Entity, &Monster, &mut gd_game_core::combat::Combatant), Added<Monster>>,
    data: Res<gd_game_core::data::GameData>,
    rules: Res<gd_game_core::rules::CombatRules>,
    // Option: `progress` is absent before the player exists ([`current_player_level`]); `loaded` is absent for
    // a new character.
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    session: crate::session::Session,
) {
    let idb = &data.item_db();
    let player_level = current_player_level(progress.as_deref(), loaded.as_deref()) as u32;
    for (e, m, mut cb) in &mut q {
        let params = session.loot(m.level.max(1) as u32, player_level);
        let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
        let (items, resolved): (Vec<_>, Vec<_>) =
            gd_sim::items::loot::monster_worn(idb, &m.record, &params, &mut rng)
                .iter()
                .map(|r| r.to_item())
                .filter_map(|i| gd_sim::items::resolve(idb, &i).map(|r| (i, r)))
                .unzip();
        if items.is_empty() {
            continue;
        }
        debug!(
            "{} wears {:?}",
            m.record.rsplit('/').next().unwrap_or(""),
            items
                .iter()
                .map(|i| i.base_name.rsplit('/').next().unwrap_or(""))
                .collect::<Vec<_>>()
        );
        if let Some(p) = gd_sim::actor::monster_full(
            &data.db,
            &rules.0,
            &m.record,
            m.level,
            gd_sim::actor::Side::Monster,
            &[],
            &resolved,
        ) {
            cb.life = p.sheet.life;
            cb.profile = std::sync::Arc::new(p);
        }
        commands.entity(e).try_insert(Worn { items, resolved });
    }
}

/// On a level root whose gameplay content [`run_proxies`] spawned (the game
/// then spawns its props and removes [`LevelProxies`]).
#[derive(Component)]
pub struct LevelPopulated;

/// Populates newly loaded levels (the streamer adds [`LevelProxies`]): set
/// pieces roll their objects once a session ([`SetPieceStates`]; their
/// creatures and proxies join the level's proxies), proxies roll their
/// monsters and accessory chests once a session ([`ProxyStates`],
/// `ChestStates::accessories`), ambushes wait for the player, the living
/// monsters queue on the root ([`PendingMonsters`], spawned by
/// [`spawn_level_monsters`]); patrol points, nav blockers and plain NPCs
/// spawn. The game spawns the level's props (chests, doors, breakables,
/// shrines, ...) after.
#[allow(clippy::too_many_arguments)]
pub fn run_proxies(
    mut commands: Commands,
    levels: Query<(Entity, &LevelProxies), Without<LevelPopulated>>,
    // Option: `progress` is absent before the player exists ([`current_player_level`]); `loaded` is absent for
    // a new character.
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    nav: Res<crate::nav::Nav>,
    mut states: ResMut<ProxyStates>,
    mut chests: ResMut<gd_game_core::world::ChestStates>,
    mut set_states: ResMut<SetPieceStates>,
    mut assets: crate::creature::Spawner,
    world: Res<gd_game_core::levels::Levels>,
    groups: Res<crate::patrol::PatrolGroups>,
    (session, mut unified, mut unique): (
        crate::session::Session,
        ResMut<UnifiedPools>,
        crate::unique::UniqueEntities,
    ),
    mut bosses: ResMut<HeroBosses>,
) {
    let player_level = current_player_level(progress.as_deref(), loaded.as_deref());
    let (mode, asc) = (session.mode(), session.ascendant());
    let difficulty = mode.difficulty;
    for (root, lp) in &levels {
        commands.entity(root).insert(LevelPopulated);
        // The level's living monsters, spawned by [`spawn_level_monsters`].
        let mut pending: Vec<PendingSpawn> = Vec::new();
        let no_patrol: std::sync::Arc<[[u32; 4]]> = std::sync::Arc::new([]);
        // Set pieces: their proxies join the level's, creatures become
        // single-monster entries.
        let mut extra = Vec::new();
        for (ei, rec, pos, yaw, uid) in &lp.set_pieces {
            let objs = set_states.0.entry((lp.level, *ei)).or_insert_with(|| {
                // A set piece its unique-entity group leaves out is spent (`crate::unique`).
                if !unique.loads(*uid, crate::unique::Loader::SetPiece) {
                    debug!("set piece {rec} {uid:?}: not chosen in its unique group");
                    return Vec::new();
                }
                let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
                // Set pieces of one "Unified Entities" group share the pool
                // choice (`SetPiece::Generate`, one store with the proxies,
                // [`UnifiedPools`]); the engine stores the chosen index after
                // every choice, overwriting (`SetEntityUnificationIndex`).
                let group = uid
                    .and_then(|u| groups.groups.unified_group(u))
                    .map(str::to_owned);
                let mut pick = group.as_ref().and_then(|g| unified.0.get(g).copied());
                let Some(run) = gd_sim::setpiece::run_set_piece_shared(
                    &assets.data.db,
                    rec,
                    gd_sim::ascendant::totem_chance(&assets.data.db, asc, mode),
                    &mut pick,
                    &mut rng,
                ) else {
                    return Vec::new();
                };
                if let (Some(g), Some(i)) = (group, pick) {
                    unified.0.insert(g, i);
                }
                let mut objs = place_set_piece(&nav.0, *pos, *yaw, &run);
                for o in &mut objs {
                    o.class = assets
                        .data
                        .db
                        .get(&o.record)
                        .map(|r| r.class.clone())
                        .unwrap_or_default();
                }
                debug!(
                    "set piece {rec}: {:?}",
                    objs.iter()
                        .map(|o| (o.record.rsplit('/').next().unwrap_or(""), o.class.as_str()))
                        .collect::<Vec<_>>()
                );
                objs
            });
            for (n, o) in objs.clone().into_iter().enumerate() {
                let key = set_key(*ei, n);
                match o.class.as_str() {
                    "Proxy" | "ProxyAmbush" => extra.push(ProxyCpu {
                        entity: key,
                        record: o.record,
                        position: o.position,
                        uid: None,
                    }),
                    "Monster" => {
                        states.0.entry((lp.level, key)).or_insert_with(|| {
                            let limits = world.level_limits(Some(lp.level), o.position, difficulty);
                            let level = gd_sim::proxy::area_level(player_level, limits);
                            vec![MonsterState {
                                record: o.record.clone(),
                                level,
                                position: o.position,
                                yaw: o.yaw,
                                variant: gd_game_core::rng::u32(..),
                                alive: true,
                                pending: false,
                                scale: None,
                            }]
                        });
                        extra.push(ProxyCpu {
                            entity: key,
                            record: o.record,
                            position: o.position,
                            uid: None,
                        });
                    }
                    // A mesh-less nav blocker (the game places the meshes).
                    "NavBlocker"
                        if assets
                            .data
                            .db
                            .get(&o.record)
                            .is_some_and(|r| r.str("mesh").is_none()) =>
                    {
                        if let Some(r) = assets.data.db.get(&o.record) {
                            let tf = Transform::from_translation(o.position)
                                .with_rotation(o.rot)
                                .with_scale(Vec3::splat(o.scale));
                            commands.spawn((
                                tf,
                                crate::nav::NavObstacle(vec![crate::obstacles::nav_blocker(
                                    r, o.position,
                                )]),
                                LevelProp(lp.level),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        for p in lp.proxies.iter().chain(&extra) {
            let key = (lp.level, p.entity);
            let list = states.0.entry(key).or_insert_with(|| {
                // A proxy its unique-entity group leaves out builds no pools: nothing spawns
                // (`crate::unique`).
                if !unique.loads(p.uid, crate::unique::Loader::Proxy) {
                    debug!(
                        "proxy {} {:?}: not chosen in its unique group",
                        p.record, p.uid
                    );
                    return Vec::new();
                }
                let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
                let limits = world.level_limits(Some(lp.level), p.position, difficulty);
                let area = gd_sim::proxy::area_level(player_level, limits);
                debug!("proxy {} limits {limits:?}: area level {area}", p.record);
                let endless = assets
                    .data
                    .db
                    .get(&p.record)
                    .is_some_and(|r| r.class == "ProxyEndless");
                // Proxies of one "Unified Entities" group share the pool choice (`Proxy::InitializePools` /
                // `ProxyEndless::InitializePools`, one store for both, [`UnifiedPools`]).
                let group = p
                    .uid
                    .and_then(|u| groups.groups.unified_group(u))
                    .map(str::to_owned);
                let mut pick = group.as_ref().and_then(|g| unified.0.get(g).copied());
                let run = if endless {
                    gd_sim::proxy::ProxyRun {
                        monsters: gd_sim::proxy::run_endless_proxy(
                            &assets.data.db,
                            &p.record,
                            player_level,
                            area,
                            mode,
                            &mut pick,
                            &bosses.0,
                            &mut rng,
                        ),
                        accessories: gd_sim::proxy::run_endless_accessory(
                            &assets.data.db,
                            &p.record,
                            mode.difficulty,
                            &mut rng,
                        )
                        .into_iter()
                        .collect(),
                        register_bosses: Vec::new(),
                    }
                } else {
                    gd_sim::proxy::run_proxy_shared(
                        &assets.data.db,
                        &p.record,
                        player_level,
                        area,
                        mode,
                        gd_sim::ascendant::trove_chance(&assets.data.db, asc, mode),
                        true,
                        &mut pick,
                        &bosses.0,
                        false,
                        &mut rng,
                    )
                };
                bosses.0.extend(run.register_bosses.iter().cloned());
                if let (Some(g), Some(i)) = (group, pick) {
                    unified.0.entry(g).or_insert(i);
                }
                let extents = gd_sim::proxy::proxy_info(&assets.data.db, &p.record)
                    .map(|i| i.placement_extents)
                    .unwrap_or(5.0);
                // Accessories (chests) are placed with the monsters.
                let points = place(
                    &nav.0,
                    p.position,
                    extents,
                    run.monsters.len() + run.accessories.len(),
                );
                let (points, extra) = points.split_at(run.monsters.len());
                let acc = run
                    .accessories
                    .into_iter()
                    .zip(extra)
                    .map(|(r, p)| (r, *p, gd_game_core::rng::f32() * std::f32::consts::TAU))
                    .collect();
                chests.accessories.insert(key, acc);
                let spawns = run.monsters;
                let points = points.to_vec();
                let ambush = assets
                    .data
                    .db
                    .get(&p.record)
                    .is_some_and(|r| r.class == "ProxyAmbush");
                spawns
                    .into_iter()
                    .zip(points)
                    .map(|(s, pos)| MonsterState {
                        record: s.record,
                        level: s.level,
                        position: pos,
                        yaw: gd_game_core::rng::f32() * std::f32::consts::TAU,
                        variant: gd_game_core::rng::u32(..),
                        alive: true,
                        pending: ambush || endless,
                        scale: None,
                    })
                    .collect()
            });
            let is_endless = assets
                .data
                .db
                .get(&p.record)
                .is_some_and(|r| r.class == "ProxyEndless");
            if is_endless {
                // Held until the generator activates it (`crate::endless`).
                commands.spawn((
                    EndlessProxy {
                        key,
                        record: p.record.clone(),
                        uid: p.uid,
                        patrol: p
                            .uid
                            .map(|u| groups.groups.for_proxy(u))
                            .unwrap_or_default(),
                    },
                    LevelProp(lp.level),
                ));
            } else if list.iter().any(|m| m.alive && m.pending)
                && let Some(r) = assets.data.db.get(&p.record)
            {
                let ms = |k: &str| (r.f32(k).unwrap_or(0.0) * 1000.0) as i32;
                debug!(
                    "ambush {} at {:?}: {} held",
                    p.record,
                    p.position,
                    list.len()
                );
                commands.spawn((
                    Ambush {
                        key,
                        position: p.position,
                        alert: r.f32("alertArea").unwrap_or(10.0),
                        spawn_ms: (ms("minSpawnTime"), ms("maxSpawnTime")),
                        delay_ms: (ms("minDelayTime"), ms("maxDelayTime")),
                        group: (
                            r.i32("minGroupSize").unwrap_or(1),
                            r.i32("maxGroupSize").unwrap_or(1),
                        ),
                        threshold: r.i32("spawnThreshold").unwrap_or(10000).max(0) as usize,
                        alerted: false,
                        timer: 0,
                    },
                    LevelProp(lp.level),
                ));
            }
            // `ControllerMonster::PostParentAttached`: the patrol points of the
            // groups holding the proxy.
            let patrol: std::sync::Arc<[[u32; 4]]> = p
                .uid
                .map(|u| groups.groups.for_proxy(u))
                .unwrap_or_default()
                .into();
            pending.extend(
                list.iter()
                    .enumerate()
                    .filter(|(_, m)| m.alive && !m.pending)
                    .map(|(index, _)| PendingSpawn {
                        key,
                        index,
                        patrol: patrol.clone(),
                    }),
            );
        }
        for (ei, rec, tf) in &lp.placed {
            let Some(r) = assets.data.db.get(rec) else {
                continue;
            };
            match r.class.as_str() {
                "CharonGeyserMarker" => {
                    commands.spawn((*tf, crate::geysers::GeyserMarker, LevelProp(lp.level)));
                }
                // A monster placed in the level: its record's level at the players' level
                // ([`placed_char_level`]), for its stats, skills and rewards.
                "Turret" => {
                    let key = (lp.level, *ei);
                    let list = states.0.entry(key).or_insert_with(|| {
                        let z = gd_game_core::character::facing(tf);
                        vec![MonsterState {
                            record: rec.clone(),
                            level: placed_char_level(&assets.data.db, rec, player_level),
                            position: tf.translation,
                            yaw: z.x.atan2(z.z),
                            variant: gd_game_core::rng::u32(..),
                            alive: true,
                            pending: false,
                            scale: None,
                        }]
                    });
                    pending.extend(list.iter().enumerate().filter(|(_, m)| m.alive).map(
                        |(index, _)| PendingSpawn {
                            key,
                            index,
                            patrol: no_patrol.clone(),
                        },
                    ));
                }
                _ => {}
            }
        }
        for (rec, pos, uid) in &lp.patrol_points {
            let Some(r) = assets.data.db.get(rec) else {
                continue;
            };
            commands.spawn((
                Transform::from_translation(*pos),
                crate::patrol::PatrolPoint {
                    uid: *uid,
                    radius: r.f32("radius").unwrap_or(0.0),
                    run: r.bool("shouldRun"),
                },
                LevelProp(lp.level),
            ));
        }
        for (rec, pos) in &lp.blockers {
            if let Some(r) = assets.data.db.get(rec) {
                commands.spawn((
                    Transform::from_translation(*pos),
                    crate::nav::NavObstacle(vec![crate::obstacles::nav_blocker(r, *pos)]),
                    LevelProp(lp.level),
                ));
            }
        }
        for (ei, rec, tf) in &lp.npcs {
            let mut tf = *tf;
            // The placement's scale (`LevelProxies::npcs`).
            let placed = Some(tf.scale.y);
            if let Some(cell) = nav
                .0
                .nearest((tf.translation + Vec3::Y * 2.0).to_array(), 3.0)
            {
                tf.translation.y = nav.0.pos(cell)[1];
            }
            // A placed `Monster` (a town guard; a faction champion) is a monster like a placed Turret: it fights by
            // faction (an ally when friendly, [`is_fighter`]) at its record's level ([`placed_char_level`]), and stays
            // dead for the session once killed.
            if is_fighter(&assets.data.db, rec) {
                let key = (lp.level, *ei);
                let list = states.0.entry(key).or_insert_with(|| {
                    let z = gd_game_core::character::facing(&tf);
                    vec![MonsterState {
                        record: rec.clone(),
                        level: placed_char_level(&assets.data.db, rec, player_level),
                        position: tf.translation,
                        yaw: z.x.atan2(z.z),
                        variant: *ei,
                        alive: true,
                        pending: false,
                        scale: placed,
                    }]
                });
                pending.extend(list.iter().enumerate().filter(|(_, m)| m.alive).map(
                    |(index, _)| PendingSpawn {
                        key,
                        index,
                        patrol: no_patrol.clone(),
                    },
                ));
                continue;
            }
            if let Some(e) =
                crate::creature::spawn_placed(&mut commands, &mut assets, tf, rec, *ei, placed)
            {
                commands
                    .entity(e)
                    .insert((LevelNpc(lp.level), Name::new("npc")));
            }
        }
        if !pending.is_empty() {
            info!(
                "level {}: {} proxies, {} monsters to spawn",
                lp.level,
                lp.proxies.len(),
                pending.len()
            );
            commands.entity(root).insert(PendingMonsters(pending));
        }
    }
}

/// Things a dying monster leaves (`spawnObjects` of its on-death skills):
/// a chest, a breakable or a creature.
///
/// `Skill_OnDeathSpawnActor::ActivateOnDeathNow`: its
/// `spawnObjects` actor appears where the monster died, on the floor (`WorldVec3::TranslateToFloor`; a
/// nemesis's loot chest, a hero's loot orb). `SpawnActor`
/// creates whatever the record is and adds it to the world at the owner's
/// level: containers, destructibles, monsters (Gilisten's
/// rebirth) and NPCs (Direni, Edmund Doyle) all appear. The game dresses
/// them like any chest, breakable or creature; the drops are keyed by [`DropSeq`].
#[allow(clippy::too_many_arguments)]
pub fn death_actors(
    mut commands: Commands,
    mut spawner: crate::creature::Spawner,
    mut fresh: gd_game_core::session::NewSession,
    mut seq: Local<DropSeq>,
    rules: Res<gd_game_core::rules::CombatRules>,
    broken: Res<gd_game_core::world::Broken>,
    nav: Res<crate::nav::Nav>,
    q: Query<
        (&Transform, &crate::monster_skills::MonsterSkills, &Monster),
        Added<gd_game_core::combat::Dead>,
    >,
) {
    if fresh.begun() {
        *seq = DropSeq::default();
    }
    for (tf, ms, m) in &q {
        for (skill, li) in &ms.on_death {
            let Some(rec) = spawner.data.db.get(skill) else {
                continue;
            };
            let Some(obj) = rec
                .strs_iter("spawnObjects")
                .nth(*li)
                .or_else(|| rec.strs_iter("spawnObjects").last())
                .map(|s| s.to_string())
            else {
                continue;
            };
            let at = tf.translation;
            let floor = nav.0.ground([at.x, at.y + 1.0, at.z]).unwrap_or(at.y);
            let t = Transform::from_translation(at.with_y(floor)).with_rotation(tf.rotation);
            if spawner
                .data
                .db
                .get(&obj)
                .is_some_and(|r| r.class.contains("Container"))
            {
                let key = gd_game_core::world::ChestKey::Accessory(usize::MAX, seq.next(), 0);
                crate::chests::spawn_chest(
                    &mut commands,
                    &spawner.data.db,
                    t,
                    &obj,
                    key,
                    m.level,
                    false,
                );
                info!(
                    "{} dropped {}",
                    m.record.rsplit('/').next().unwrap_or(""),
                    obj.rsplit('/').next().unwrap_or("")
                );
            } else if spawner
                .data
                .db
                .get(&obj)
                .is_some_and(|r| r.class == "Destructible")
            {
                crate::breakables::spawn_breakable(
                    &mut commands,
                    &spawner.data,
                    &mut spawner.bodies,
                    &rules,
                    &broken,
                    t,
                    &obj,
                    (usize::MAX, seq.next()),
                    m.level,
                );
                info!(
                    "{} dropped {}",
                    m.record.rsplit('/').next().unwrap_or(""),
                    obj.rsplit('/').next().unwrap_or("")
                );
            } else if let Some((npc, radius)) = spawner
                .data
                .db
                .get(&obj)
                .filter(|r| r.class == "Monster" || r.class == "Npc")
                .map(|r| {
                    (
                        r.class == "Npc",
                        gd_game_core::combat::get_radius(Some(r), 0.5),
                    )
                })
            {
                let Some(ne) = crate::creature::spawn(
                    &mut commands,
                    &mut spawner,
                    t,
                    &obj,
                    gd_game_core::rng::u32(..),
                ) else {
                    continue;
                };
                if npc {
                    commands
                        .entity(ne)
                        .insert((LevelNpc(m.slot.0), Name::new("npc")));
                } else if let Some(profile) = gd_sim::actor::monster(
                    &spawner.data.db,
                    &rules.0,
                    &obj,
                    m.level,
                    gd_sim::actor::Side::Monster,
                ) {
                    commands.entity(ne).insert((
                        Monster {
                            record: obj.clone(),
                            level: m.level,
                            slot: (m.slot.0, u32::MAX, usize::MAX),
                        },
                        gd_game_core::combat::Combatant::new(std::sync::Arc::new(profile), radius),
                        crate::combat::MonsterAi::from_record(&spawner.data, &obj, t.translation),
                    ));
                }
                info!(
                    "{} spawned {}",
                    m.record.rsplit('/').next().unwrap_or(""),
                    obj.rsplit('/').next().unwrap_or("")
                );
            } else {
                debug!("on-death actor {obj} not spawned");
            }
        }
    }
}

/// Keys for on-death drops: a session counter, not the dead monster's entity
/// index. Bevy recycles indices, and `Broken` / the chest states last the
/// session: a later monster reusing a dead one's index must not inherit its
/// broken or opened drop.
#[derive(Default)]
pub struct DropSeq(u32);

impl DropSeq {
    fn next(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

pub struct MonstersPlugin;

impl Plugin for MonstersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SetPieceStates>()
            .init_resource::<HeroBosses>()
            .init_resource::<gd_game_core::world::ChestStates>()
            .add_systems(
                Update,
                (
                    run_proxies
                        .in_set(MonsterStage::Proxies)
                        .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                        .after(crate::factions::FactionStage::Story),
                    spawn_level_monsters
                        .in_set(MonsterStage::Proxies)
                        .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                        .after(run_proxies),
                    spawn_level_objects
                        .in_set(MonsterStage::LevelObjects)
                        .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                        .after(run_proxies),
                    ambushes.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    cull_monsters,
                    // Its fresh profile (no buffs) must land before the auras and the
                    // monster profile rebuild, or it wipes the buffs they added in
                    // the monster's first frame (the rebuild consumed the dirty flag).
                    equip_monsters
                        .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                        .before(crate::auras::AurasSet)
                        .before(crate::monster_skills::MonsterSkillStage::Rebuild),
                    death_actors
                        .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                        .after(crate::monster_skills::MonsterSkillStage::Dying)
                        .before(crate::monster_skills::MonsterSkillStage::Generators),
                ),
            );
    }
}

/// Runs a script-placed proxy (`LuaProxy::Create` + `SetCoords` from Lua or a
/// quest) at `position` and spawns its monsters; `id` is the story object, so
/// they are story monsters (slot level `usize::MAX`) left to the story to
/// despawn. Same rules as level proxies, without the per-session memory.
/// `limits_at` is `Proxy.Create`'s vec (`LuaProxy::CreateAt` → `Proxy::SetLimitsLocation`): the area limits
/// are read there, the monsters still place at `position`; `ignore_boss` its bool
/// (`CreateAtIgnoreBoss`, byte +1612): the bosses spawn without registering in [`HeroBosses`].
#[allow(clippy::too_many_arguments)]
pub fn spawn_story_proxy(
    commands: &mut Commands,
    data: &gd_game_core::data::GameData,
    bodies: &mut gd_game_core::creature::BodyCache,
    nav: &crate::nav::Nav,
    rules: &gd_game_core::rules::CombatRules,
    world: Option<&gd_game_core::levels::Levels>,
    record: &str,
    position: Vec3,
    player_level: i32,
    session: &crate::session::Session,
    factions: Option<&crate::factions::PlayerFactions>,
    id: u32,
    patrol: &[[u32; 4]],
    limits_at: Option<Vec3>,
    ignore_boss: bool,
    bosses: &mut HeroBosses,
) -> Vec<Entity> {
    let mut rng = Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    let mode = session.mode();
    // PORT: a script-placed proxy has no region here (engine source not traced): position lookup
    // (`SetLimitsLocation`'s vec when the script gave one).
    let limits =
        world.and_then(|w| w.level_limits(None, limits_at.unwrap_or(position), mode.difficulty));
    let area = gd_sim::proxy::area_level(player_level, limits);
    let trove = gd_sim::ascendant::trove_chance(&data.db, session.ascendant(), mode);
    let run = gd_sim::proxy::run_proxy(
        &data.db,
        record,
        player_level,
        area,
        mode,
        trove,
        true,
        &bosses.0,
        ignore_boss,
        &mut rng,
    );
    bosses.0.extend(run.register_bosses.iter().cloned());
    let spawns = run.monsters;
    let extents = gd_sim::proxy::proxy_info(&data.db, record)
        .map(|i| i.placement_extents)
        .unwrap_or(5.0);
    let mut points = place(
        &nav.0,
        position,
        extents,
        spawns.len() + run.accessories.len(),
    );
    // Its accessory pool's chests (as for level proxies), keyed by the story id.
    let extra = points.split_off(spawns.len().min(points.len()));
    for (n, (rec, p)) in run.accessories.iter().zip(extra).enumerate() {
        let tf = Transform::from_translation(p).with_rotation(Quat::from_rotation_y(
            gd_game_core::rng::f32() * std::f32::consts::TAU,
        ));
        crate::chests::spawn_chest(
            commands,
            &data.db,
            tf,
            rec,
            gd_game_core::world::ChestKey::Accessory(usize::MAX, id, n),
            player_level,
            false,
        );
    }
    let mut out = Vec::new();
    for (i, (m, pos)) in spawns.into_iter().zip(points).enumerate() {
        let tf = Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(
            gd_game_core::rng::f32() * std::f32::consts::TAU,
        ));
        let Some((e, _)) = crate::creature::spawn_creature(
            commands,
            data,
            bodies,
            tf,
            &m.record,
            gd_game_core::rng::u32(..),
            None,
        ) else {
            continue;
        };
        if stands_as_npc(&data.db, factions, &m.record) {
            out.push(e);
            continue;
        }
        if is_friendly(&data.db, factions, &m.record) {
            commands.entity(e).insert(crate::combat::Allied);
        }
        commands.entity(e).insert(Monster {
            record: m.record.clone(),
            level: m.level,
            slot: (usize::MAX, id, i),
        });
        if let Some(p) = gd_sim::actor::monster(
            &data.db,
            &rules.0,
            &m.record,
            m.level,
            gd_sim::actor::Side::Monster,
        ) {
            let radius = gd_game_core::combat::get_radius(data.db.get(&m.record), 0.5);
            let mut ai = crate::combat::MonsterAi::from_record(data, &m.record, pos);
            ai.patrol.replace(patrol.to_vec());
            commands.entity(e).insert((
                gd_game_core::combat::Combatant::new(std::sync::Arc::new(p), radius),
                ai,
            ));
        }
        out.push(e);
    }
    info!(
        "story proxy {id} {}: {} monsters",
        record.rsplit('/').next().unwrap_or(""),
        out.len()
    );
    out
}

/// The level's gameplay objects are spawned ([`spawn_level_objects`]).
#[derive(Component)]
pub struct LevelObjectsSpawned;

/// A newly populated level's gameplay objects, after [`run_proxies`]:
/// set-piece breakables and chests, the nav obstacles of the other set-piece
/// objects (a blocking one's mesh hit boxes, like `FixedItem::CreatePathObstacle`;
/// a `NavBlocker` whose mesh fails to load its cylinder), proxy
/// accessory chests, riftgates and respawn points, devotion shrines and the
/// ascendant altar, breakables, doors and level chests. The game adds what it
/// only shows (set-piece meshes, sound emitters), then drops the [`LevelProxies`].
///
/// PORT: a level's objects spawn once every mesh and door animation they read is parsed in the background
/// ([`level_objects_ready`]), not in the frame the level is populated, so walking into a level does not parse them on
/// the game thread. While it waits the level holds the meshes already parsed (`waiting`): `BodyCache` only keeps its
/// last `gd_game_core::weak_cache::RECENT` meshes warm, so a level with many set pieces beside a crowd would otherwise lose
/// its early meshes before the last one arrives.
#[allow(clippy::too_many_arguments)]
pub fn spawn_level_objects(
    mut commands: Commands,
    levels: Query<(Entity, &LevelProxies), (With<LevelPopulated>, Without<LevelObjectsSpawned>)>,
    // Option: `progress` is absent before the player exists ([`current_player_level`]); `loaded` is absent for
    // a new character; the codex is optional input.
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    nav: Res<crate::nav::Nav>,
    chests: Res<gd_game_core::world::ChestStates>,
    codex: Option<Res<crate::lore::LoreCodex>>,
    set_states: Res<SetPieceStates>,
    broken: Res<gd_game_core::world::Broken>,
    (data, mut bodies, mut anims): (
        Res<gd_game_core::data::GameData>,
        ResMut<gd_game_core::creature::BodyCache>,
        ResMut<gd_game_core::character::AnimCache>,
    ),
    rules: Res<gd_game_core::rules::CombatRules>,
    mode: Res<gd_game_core::session::GameMode>,
    mut waiting: Local<HashMap<Entity, Vec<std::sync::Arc<gd_formats::msh::Mesh>>>>,
) {
    waiting.retain(|e, _| levels.contains(*e));
    let player_level = current_player_level(progress.as_deref(), loaded.as_deref());
    for (root, lp) in &levels {
        // This frame's holds replace last frame's only after they were taken again.
        let mut held = Vec::new();
        if !level_objects_ready(&data, &mut bodies, &mut anims, lp, &set_states, &mut held) {
            waiting.insert(root, held);
            continue;
        }
        // The spawned objects hold their meshes now.
        waiting.remove(&root);
        commands.entity(root).insert(LevelObjectsSpawned);
        for (ei, ..) in &lp.set_pieces {
            let Some(objs) = set_states.0.get(&(lp.level, *ei)) else {
                continue;
            };
            for (n, o) in objs.iter().enumerate() {
                let key = set_key(*ei, n);
                let tf = Transform::from_translation(o.position)
                    .with_rotation(o.rot)
                    .with_scale(Vec3::splat(o.scale));
                match o.class.as_str() {
                    "Destructible" => {
                        crate::breakables::spawn_breakable(
                            &mut commands,
                            &data,
                            &mut bodies,
                            &rules,
                            &broken,
                            tf,
                            &o.record,
                            (lp.level, key),
                            player_level,
                        );
                    }
                    "FixedItemContainer" => {
                        let ck = gd_game_core::world::ChestKey::Accessory(lp.level, key, 0);
                        crate::chests::spawn_chest(
                            &mut commands,
                            &data.db,
                            tf,
                            &o.record,
                            ck,
                            player_level,
                            chests.opened.contains(&ck),
                        );
                    }
                    // A set-piece proxy's accessory chests, rolled by [`run_proxies`] under its set key.
                    "Proxy" | "ProxyAmbush" => spawn_accessories(
                        &mut commands,
                        &data.db,
                        &chests,
                        (lp.level, key),
                        player_level,
                    ),
                    "Monster" => {}
                    _ => {
                        let Some(r) = data.db.get(&o.record) else {
                            continue;
                        };
                        let Some(mesh) = r.str("mesh") else { continue };
                        let obstacles = match bodies.get(&data, mesh) {
                            Some(msh)
                                if o.block
                                    || (r.class == "Decoration" && !r.bool("allowPathing")) =>
                            {
                                crate::obstacles::hit_box_obstacles(&msh, &tf)
                            }
                            Some(_) => Vec::new(),
                            None if o.class == "NavBlocker" => {
                                vec![crate::obstacles::nav_blocker(r, o.position)]
                            }
                            None => Vec::new(),
                        };
                        if !obstacles.is_empty() {
                            commands.spawn((
                                tf,
                                crate::nav::NavObstacle(obstacles),
                                LevelProp(lp.level),
                            ));
                        }
                    }
                }
            }
        }
        for p in &lp.proxies {
            spawn_accessories(
                &mut commands,
                &data.db,
                &chests,
                (lp.level, p.entity),
                player_level,
            );
        }
        for (rec, pos, uid) in &lp.waypoints {
            let Some(r) = data.db.get(rec) else { continue };
            // The Chthonic rifts (`crate::riftgates::DynamicRift`).
            if r.class == "DynamicTeleporter" {
                let e = crate::riftgates::spawn_dynamic_rift(&mut commands, r, rec, *pos, *uid);
                commands.entity(e).insert(LevelProp(lp.level));
                continue;
            }
            let Some(kind) = crate::riftgates::waypoint_kind(&r.class) else {
                continue;
            };
            let radius = match kind {
                crate::riftgates::WaypointKind::Riftgate => r.f32("radius").unwrap_or(3.0),
                crate::riftgates::WaypointKind::Respawner => r.f32("bindRadius").unwrap_or(8.0),
            };
            let mut e = commands.spawn((
                Transform::from_translation(*pos),
                crate::riftgates::Waypoint {
                    kind,
                    uid: *uid,
                    record: rec.to_ascii_lowercase(),
                    radius,
                    bind_radius: r.f32("bindRadius").unwrap_or(0.0),
                    level: lp.level,
                    locked: r.bool("locked"),
                },
                LevelProp(lp.level),
            ));
            // Only a riftgate is used (a respawner binds by walking into its radius).
            if kind == crate::riftgates::WaypointKind::Riftgate {
                e.insert(crate::player_orders::Interactable::fixed(radius.min(2.0)));
            }
        }
        for (rec, tf, uid) in &lp.shrines {
            if data
                .db
                .get(rec)
                .is_some_and(|r| r.class == "AscendantAltar")
            {
                if let Some(e) = crate::ascendant::spawn_altar(&mut commands, &data.db, *tf, rec) {
                    commands.entity(e).insert(LevelProp(lp.level));
                }
                continue;
            }
            if let Some(e) =
                crate::devotion::spawn_shrine(&mut commands, &data.db, *tf, rec, *uid, mode.0)
            {
                commands.entity(e).insert(LevelProp(lp.level));
            }
        }
        for (ei, rec, tf) in &lp.breakables {
            crate::breakables::spawn_breakable(
                &mut commands,
                &data,
                &mut bodies,
                &rules,
                &broken,
                *tf,
                rec,
                (lp.level, *ei),
                player_level,
            );
        }
        for (rec, tf, uid) in &lp.doors {
            crate::doors::spawn_door(
                &mut commands,
                &data,
                &mut bodies,
                &mut anims,
                *tf,
                rec,
                *uid,
                Some(lp.level),
                None,
            );
        }
        for (ei, rec, tf) in &lp.chests {
            let key = gd_game_core::world::ChestKey::Level(lp.level, *ei);
            if data.db.get(rec).is_some_and(|r| r.class == "ItemNote") {
                let at = nav
                    .0
                    .ground(tf.translation.to_array())
                    .map_or(tf.translation, |y| tf.translation.with_y(y));
                if let Some(e) = crate::lore::spawn_placed_note(
                    &mut commands,
                    rec,
                    at,
                    key,
                    chests.opened.contains(&key) || codex.as_deref().is_some_and(|c| c.knows(rec)),
                ) {
                    commands.entity(e).insert(LevelProp(lp.level));
                }
                continue;
            }
            let mut tf = *tf;
            if let Some(cell) = nav
                .0
                .nearest((tf.translation + Vec3::Y * 2.0).to_array(), 3.0)
            {
                tf.translation.y = tf.translation.y.max(nav.0.pos(cell)[1] - 0.5);
            }
            crate::chests::spawn_chest(
                &mut commands,
                &data.db,
                tf,
                rec,
                key,
                0,
                chests.opened.contains(&key),
            );
        }
    }
}

/// Spawns the accessory chests [`run_proxies`] rolled for the proxy `(level, entity)`.
fn spawn_accessories(
    commands: &mut Commands,
    db: &gd_formats::vfs::Database,
    chests: &gd_game_core::world::ChestStates,
    (level, entity): (usize, u32),
    player_level: i32,
) {
    for (i, (rec, pos, yaw)) in chests
        .accessories
        .get(&(level, entity))
        .into_iter()
        .flatten()
        .enumerate()
    {
        // A buff shrine is not a chest: `crate::endless::spawn_activated_shrines` places the ones of the
        // proxies the generator activated.
        if db.get(rec).is_some_and(|r| r.class == "EndlessBuffShrine") {
            continue;
        }
        let key = gd_game_core::world::ChestKey::Accessory(level, entity, i);
        let tf = Transform::from_translation(*pos).with_rotation(Quat::from_rotation_y(*yaw));
        crate::chests::spawn_chest(
            commands,
            db,
            tf,
            rec,
            key,
            player_level,
            chests.opened.contains(&key),
        );
    }
}

/// Whether every mesh and door animation [`spawn_level_objects`] reads for the level is parsed (one not yet parsed
/// starts its background load: [`gd_game_core::creature::BodyCache::ready`], [`gd_game_core::character::AnimCache::ready`]): the
/// set-piece objects', the breakables' and the doors'. The object meshes already parsed go into `held`.
fn level_objects_ready(
    data: &gd_game_core::data::GameData,
    bodies: &mut gd_game_core::creature::BodyCache,
    anims: &mut gd_game_core::character::AnimCache,
    lp: &LevelProxies,
    set_states: &SetPieceStates,
    held: &mut Vec<std::sync::Arc<gd_formats::msh::Mesh>>,
) -> bool {
    let mut mesh = |bodies: &mut gd_game_core::creature::BodyCache, rec: &str| {
        let Some(m) = data.db.get(rec).and_then(|r| r.str("mesh")) else {
            return true;
        };
        if !bodies.ready(data, m) {
            return false;
        }
        held.extend(bodies.loaded(&gd_formats::vfs::normalized(m)).flatten());
        true
    };
    let mut ready = true;
    for (ei, ..) in &lp.set_pieces {
        for o in set_states.0.get(&(lp.level, *ei)).into_iter().flatten() {
            if !matches!(
                o.class.as_str(),
                "Proxy" | "ProxyAmbush" | "Monster" | "FixedItemContainer"
            ) {
                ready &= mesh(&mut *bodies, &o.record);
            }
        }
    }
    for (_, rec, _) in &lp.breakables {
        ready &= mesh(&mut *bodies, rec);
    }
    for (rec, ..) in &lp.doors {
        ready &= crate::doors::door_ready(data, bodies, anims, rec);
    }
    ready
}

/// Level population: `Proxies` populates a new level, `LevelObjects` spawns its gameplay objects.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonsterStage {
    Proxies,
    LevelObjects,
}
