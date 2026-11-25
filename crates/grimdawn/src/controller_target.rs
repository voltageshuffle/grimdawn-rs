//! Keyboard Only's controller targeting, the game side (rule: `gd_game::controller_target`): every frame in
//! `movementType` 2 the candidates around the player are gathered, [`ControllerPick`] keeps the pick (the hover
//! target, `hover.rs`; the ground label selection, `loot.rs`) and gd-game gets the controller's cursor record
//! (`ControllerTarget`: the picked enemy or NPC and its coordinates, else an empty record).
//!
//! Candidates (`InGameUI::CollectEnemyCandidates`): foes of the player that are targetable
//! (`Monster::IsTargetable`), not hiding (`Character::IsHiding`) and, for the combat flag, `causesAnger`;
//! targetable breakables, ground items whose label is shown, NPCs (record `Class` `Npc*`, available for
//! conversations, visible) with `Character::GetUsableRange` (both radii + 3 m), and the interactable fixed actors
//! with `FixedActor::GetUsableRange` (both radii + the record's `usableRange`): unopened chests, doors and levers
//! of interest, devotion shrines and monster totems, riftgates, the Ascendant Altar, the player's gravestone, open
//! personal portals, the Shattered Realm's exit gates and unused buff shrines; each within the camera's view
//! (`GameEngine::GetFrustumForPlayer`). "Interactable" is `Object::IsOfInterest`.
//!
//! The marker (`GamepadTargetMarker::Update` / `Position` / `Load` / `Render`, when `movementType` is 2):
//! over the picked enemy `records/ui/gamepadhud/gamepad_enemytargetbitmap.dbr`, over a picked
//! NPC or fixed actor `gamepad_friendlytargetbitmap.dbr`, its size the texture's x the zoom scale
//! (1.2 − 0.4 x (camera distance − min) / (max − min)), centred on the object's position raised by its mesh top
//! (+ `worldDescOffset`), projected and floored, then moved up by (`interactIconPosition` + 50 for a character,
//! 70 for a breakable or other object, 0 for a riftgate) x the zoom scale and bobbing by
//! `sin(ms x 0.004) x 5` x the zoom scale. No marker over a ground item (its label is selected).
//!
//! PORT: the windows that stop the targeting (`Ui::IsAnyWindowOpen`: any window or dialog) are the riftgate map, a
//! conversation and the altar menu; a picked NPC is sent as an empty-target record at its coordinates (the
//! engine's combat ally; the port's skills take only pets as friends); the frustum is the camera's view volume;
//! the mesh top is the record's mesh bounds x its `scale`.
use crate::record_facts::RecordFacts;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::collections::HashMap;
use gd_formats::ui::{Rect as URect, Widget};
use gd_game::controller_target::{
    Candidate, ControllerTarget, FixedKind, Kind, Pick, SCAN_RADIUS, Targeting, pick,
};
use gd_game_core::data::GameData;
use gd_game_core::player::Player;
use gd_ui_kit::text::FontCache;

pub struct ControllerTargetPlugin;

impl Plugin for ControllerTargetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControllerPick>()
            .add_systems(
                Update,
                (targeting, interact_key)
                    .in_set(gd_ui::state::casting::CastInputStage::Cursor)
                    .after(gd_game_core::inventory::InventorySet),
            )
            .add_systems(
                PostUpdate,
                marker
                    .after(bevy::transform::TransformSystems::Propagate)
                    .in_set(gd_ui_kit::UiPlaySet),
            );
    }
}

/// This frame's pick (Keyboard Only; `Pick::None` otherwise) and what the marker needs of it.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct ControllerPick(pub Pick, pub Option<MarkerFacts>);

#[derive(Clone, PartialEq, Debug)]
pub struct MarkerFacts {
    pub record: String,
    /// 50 for a character, 70 for a breakable or another object, 0 for a riftgate.
    pub lift: f32,
    pub enemy: bool,
}

const ENEMY_BITMAP: &str = "records/ui/gamepadhud/gamepad_enemytargetbitmap.dbr";
const FRIEND_BITMAP: &str = "records/ui/gamepadhud/gamepad_friendlytargetbitmap.dbr";

#[derive(Clone, Copy)]
struct RecFacts {
    npc: bool,
    blast: bool,
    radius: f32,
    usable: f32,
    invincible: bool,
}

fn rec_facts(data: &GameData, fonts: &FontCache, facts: &mut RecordFacts, rec: &str) -> RecFacts {
    let f = facts.of(data, fonts, rec);
    RecFacts {
        npc: f.npc,
        blast: f.blast,
        radius: f.radius,
        usable: f.usable,
        invincible: f.invincible,
    }
}

type Foes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GlobalTransform,
        &'static gd_render::character::Character,
        Option<&'static gd_game::combat::MonsterAi>,
        Option<&'static gd_game::pets::Pet>,
        gd_game::combat::VictimSide,
        Has<gd_game::combat::Invincible>,
    ),
    (Without<gd_game_core::combat::Dead>, Without<Player>),
>;
type Npcs<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GlobalTransform,
        &'static gd_render::character::Character,
        Option<&'static gd_game_core::story::StoryActor>,
        Option<&'static Visibility>,
    ),
    (
        Without<gd_game_core::monsters::Monster>,
        Without<gd_game::pets::Pet>,
        Without<Player>,
        Without<gd_game_core::combat::Dead>,
        Without<crate::doll::DollBody>,
    ),
>;
type Fixed<'w, 's> = (
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static crate::chests::Chest,
        ),
    >,
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static gd_game::doors::Door,
        ),
    >,
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static gd_game::devotion::Shrine,
        ),
    >,
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static gd_game::riftgates::Waypoint,
        ),
    >,
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static gd_game::ascendant::Altar,
        ),
    >,
    Query<'w, 's, (Entity, &'static GlobalTransform), With<gd_game::death::Gravestone>>,
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static gd_game::riftgates::Portal,
        ),
    >,
    Query<'w, 's, (Entity, &'static GlobalTransform), With<gd_game::endless::EndlessGate>>,
    Query<
        'w,
        's,
        (
            Entity,
            &'static GlobalTransform,
            &'static gd_game::endless::EndlessShrine,
        ),
    >,
);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn targeting(
    mut fresh: gd_game_core::session::NewSession,
    options: Res<gd_game_core::options::GameOptions>,
    time: Res<Time>,
    data: Res<GameData>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    players: Query<
        (
            &GlobalTransform,
            &gd_game_core::combat::Combatant,
            &gd_game_core::player::ControllerDirection,
        ),
        (With<Player>, Without<gd_game_core::combat::Dead>),
    >,
    (foes, breakables, items, npcs): (
        Foes,
        Query<
            (Entity, &GlobalTransform, &gd_game_core::world::Breakable),
            Without<gd_game_core::combat::Dead>,
        >,
        Query<
            (Entity, &GlobalTransform),
            (
                With<crate::loot::GroundItem>,
                Without<crate::loot::Unpickable>,
                Without<gd_ui::state::loot::FilteredOut>,
            ),
        >,
        Npcs,
    ),
    fixed: Fixed,
    devotion: Option<Res<gd_game::devotion::DevotionState>>,
    (rift_map, dialog, altar): (
        Res<gd_game::riftgates::RiftgateMap>,
        Res<gd_game::conversation::Conversation>,
        Res<gd_ui::state::ascendant::AltarMenu>,
    ),
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut state: Local<Targeting>,
    (mut facts, fonts): (ResMut<RecordFacts>, Res<FontCache>),
    mut current: ResMut<ControllerPick>,
    mut out: MessageWriter<ControllerTarget>,
) {
    if fresh.begun() {
        *state = Targeting::default();
    }
    if options.int("movementType") != 2 {
        current.set_if_neq(ControllerPick::default());
        return;
    }
    // `Ui::IsAnyWindowOpen`: the record stays.
    if rift_map.open || dialog.view.is_some() || altar.open {
        return;
    }
    let (Ok((pgt, pcb, controller)), Ok((cam, cam_gt))) = (players.single(), cams.single()) else {
        current.set_if_neq(ControllerPick::default());
        return;
    };
    let me = pgt.translation();
    let near = |p: Vec3| {
        (p - me).with_y(0.0).length() <= SCAN_RADIUS
            && cam
                .world_to_ndc(cam_gt, p + Vec3::Y * 0.5)
                .is_some_and(|n| n.x.abs() <= 1.0 && n.y.abs() <= 1.0 && (0.0..=1.0).contains(&n.z))
    };
    let mut fact = |rec: &str| rec_facts(&data, &fonts, &mut facts, rec);
    let story = rt.as_ref().map(|rt| rt.story.borrow());
    let available = |a: Option<&gd_game_core::story::StoryActor>| {
        a.and_then(|a| story.as_ref()?.entities.get(a.0))
            .is_none_or(|o| o.in_world && o.available_for_conversation)
    };
    // The marker's facts are built for the pick only.
    let mut list: Vec<(Candidate, &str, f32)> = Vec::new();
    let mut add = |entity: Entity, pos: Vec3, kind: Kind, record, lift: f32| {
        if near(pos) {
            list.push((Candidate { entity, pos, kind }, record, lift));
        }
    };
    let r = pcb.radius;
    for (e, gt, ch, ai, pet, victim, invincible) in &foes {
        let kind = foe_kind(
            victim.side.side_ref(),
            ai,
            pet,
            victim.hiding(),
            invincible,
            || fact(&ch.record).invincible,
        );
        if let Some(kind) = kind {
            add(e, gt.translation(), kind, &ch.record, 50.0);
        }
    }
    for (e, gt, b) in &breakables {
        if b.targetable {
            add(e, gt.translation(), Kind::Breakable, &b.record, 70.0);
        }
    }
    for (e, gt) in &items {
        add(e, gt.translation(), Kind::Item, "", 0.0);
    }
    for (e, gt, ch, actor, vis) in &npcs {
        let f = fact(&ch.record);
        if f.npc && available(actor) && vis != Some(&Visibility::Hidden) {
            let usable = f.radius + r + 3.0;
            add(e, gt.translation(), Kind::Npc { usable }, &ch.record, 50.0);
        }
    }
    for (c, record, lift) in fixed_candidates(&fixed, &data, devotion.as_deref(), r, &mut fact) {
        add(c.entity, c.pos, c.kind, record, lift);
    }
    let candidates: Vec<Candidate> = list.iter().map(|c| c.0).collect();
    let p = pick(
        me,
        controller.aim,
        &candidates,
        &mut state,
        time.elapsed_secs(),
    );
    let found = list.iter().find(|c| Some(c.0.entity) == p.entity());
    let record = match (p, found) {
        (Pick::Enemy(e), Some(c)) => ControllerTarget {
            target: Some(e),
            point: Some(c.0.pos),
        },
        (Pick::Npc(_), Some(c)) => ControllerTarget {
            target: None,
            point: Some(c.0.pos),
        },
        _ => ControllerTarget {
            target: None,
            point: None,
        },
    };
    out.write(record);
    let marker = found
        .filter(|_| !matches!(p, Pick::Item(_)))
        .map(|&(c, record, lift)| MarkerFacts {
            record: record.to_owned(),
            lift,
            enemy: matches!(c.kind, Kind::Foe { .. } | Kind::Breakable),
        });
    current.set_if_neq(ControllerPick(p, marker));
}

/// A character's foe candidate (`InGameUI::CollectEnemyCandidates`); `record_invincible` is asked only for one
/// that is invincible now.
fn foe_kind(
    side: gd_game::combat::SideRef,
    ai: Option<&gd_game::combat::MonsterAi>,
    pet: Option<&gd_game::pets::Pet>,
    hiding: bool,
    invincible: bool,
    record_invincible: impl FnOnce() -> bool,
) -> Option<Kind> {
    if !gd_game::combat::Side::Player.foe(side) {
        return None;
    }
    // `Pet`'s vtable has `Monster::IsTargetable` and `Character::CausesAnger`.
    let (targetable, causes_anger) = match (ai, pet) {
        (Some(ai), _) => (ai.as_target.targetable, ai.as_target.causes_anger),
        (None, Some(p)) => (p.targetable, p.causes_anger),
        (None, None) => return None,
    };
    // `Monster::IsTargetable`: unless the record is `invincible`, not invincible now.
    let targetable = targetable && (!invincible || record_invincible());
    // `Character::IsHiding`, and a hidden monster's Startup.
    let hidden = hiding || ai.is_some_and(|ai| ai.hidden.is_some());
    (targetable && !hidden).then_some(Kind::Foe { causes_anger })
}

/// The interactable fixed actors (`Object::IsOfInterest`) as candidates with their usable range
/// (`r`: the player's radius), their record and marker lift.
fn fixed_candidates<'a>(
    (chests, doors, shrines, gates, altars, graves, portals, exits, buffs): &'a Fixed,
    data: &GameData,
    devotion: Option<&gd_game::devotion::DevotionState>,
    r: f32,
    fact: &mut impl FnMut(&str) -> RecFacts,
) -> Vec<(Candidate, &'a str, f32)> {
    let mut out = Vec::new();
    let mut add = |entity: Entity,
                   gt: &GlobalTransform,
                   kind: FixedKind,
                   usable: f32,
                   record: &'a str,
                   lift: f32| {
        let kind = Kind::Fixed { kind, usable };
        out.push((
            Candidate {
                entity,
                pos: gt.translation(),
                kind,
            },
            record,
            lift,
        ));
    };
    for (e, gt, c) in chests.iter() {
        if c.is_of_interest() {
            let f = fact(&c.record);
            let kind = if f.blast {
                FixedKind::Shrine
            } else {
                FixedKind::Container
            };
            add(e, gt, kind, c.radius + r + f.usable, &c.record, 70.0);
        }
    }
    for (e, gt, d) in doors.iter() {
        if data
            .db
            .get(&d.record)
            .is_some_and(|rec| d.is_of_interest(rec))
        {
            let usable = d.radius + r + fact(&d.record).usable;
            add(e, gt, FixedKind::Other, usable, &d.record, 70.0);
        }
    }
    for (e, gt, s) in shrines.iter() {
        if s.is_of_interest(devotion) {
            let usable = s.radius + r + fact(&s.record).usable;
            add(e, gt, FixedKind::Shrine, usable, &s.record, 70.0);
        }
    }
    for (e, gt, w) in gates.iter() {
        if w.kind == gd_game::riftgates::WaypointKind::Riftgate && w.is_of_interest() {
            let usable = w.radius + r + fact(&w.record).usable;
            add(e, gt, FixedKind::Other, usable, &w.record, 0.0);
        }
    }
    // `AscendantAltar::IsOfInterest`: dormant (1, 2) or activated (4) and not `AscendantAltar.blocked` (PORT:
    // always, that flag not traced).
    for (e, gt, a) in altars.iter() {
        let f = fact(&a.record);
        add(
            e,
            gt,
            FixedKind::Other,
            f.radius + r + f.usable,
            &a.record,
            70.0,
        );
    }
    // `FixedItemGravestone::IsOfInterest`: the main player's grave.
    let grave = gd_game::death::GRAVE;
    for (e, gt) in graves.iter() {
        let f = fact(grave);
        add(
            e,
            gt,
            FixedKind::Other,
            f.radius + r + f.usable,
            grave,
            70.0,
        );
    }
    // `FixedItemTeleport::IsOfInterest` (also `FixedItemDungeonTeleport`): not closing.
    let portal = gd_game::riftgates::PORTAL_RECORD;
    for (e, gt, p) in portals.iter() {
        if p.is_of_interest() {
            let f = fact(portal);
            add(
                e,
                gt,
                FixedKind::Other,
                f.radius + r + f.usable,
                portal,
                70.0,
            );
        }
    }
    let exit = gd_game::endless::EXIT_GATE_RECORD;
    for (e, gt) in exits.iter() {
        let f = fact(exit);
        add(e, gt, FixedKind::Other, f.radius + r + f.usable, exit, 70.0);
    }
    for (e, gt, s) in buffs.iter() {
        if s.is_of_interest() {
            let usable = s.radius + r + fact(&s.record).usable;
            add(e, gt, FixedKind::Other, usable, &s.record, 70.0);
        }
    }
    out
}

/// The Interact key (key action 54, `gd_game::controller_target::interact_pick`).
/// PORT: NPCs the port keeps hidden are left out.
#[allow(clippy::too_many_arguments)]
fn interact_key(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    capture: Res<gd_ui::state::inventory::UiCapture>,
    data: Res<GameData>,
    players: Query<
        (&GlobalTransform, &gd_game_core::combat::Combatant),
        (With<Player>, Without<gd_game_core::combat::Dead>),
    >,
    npcs: Npcs,
    fixed: Fixed,
    devotion: Option<Res<gd_game::devotion::DevotionState>>,
    (mut facts, fonts): (ResMut<RecordFacts>, Res<FontCache>),
    mut out: MessageWriter<gd_game::controller_target::UseObject>,
) {
    if capture.0 || !bindings.just_pressed(gd_ui_kit::keys::INTERACT, &keys) {
        return;
    }
    let Ok((pgt, pcb)) = players.single() else {
        return;
    };
    let me = pgt.translation();
    let mut fact = |rec: &str| rec_facts(&data, &fonts, &mut facts, rec);
    let mut list: Vec<Candidate> =
        fixed_candidates(&fixed, &data, devotion.as_deref(), pcb.radius, &mut fact)
            .into_iter()
            .map(|c| c.0)
            .collect();
    for (e, gt, ch, _, vis) in &npcs {
        if fact(&ch.record).npc && vis != Some(&Visibility::Hidden) {
            list.push(Candidate {
                entity: e,
                pos: gt.translation(),
                kind: Kind::Npc { usable: 0.0 },
            });
        }
    }
    if let Some(e) = gd_game::controller_target::interact_pick(me, &list) {
        debug!("interact: {e:?}");
        out.write(gd_game::controller_target::UseObject(e));
    }
}

#[derive(Component)]
struct MarkerRoot;

#[allow(clippy::too_many_arguments)]
fn marker(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    current: Res<ControllerPick>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    cams: Query<(&Camera, &GlobalTransform, &gd_render::camera::GameCamera)>,
    objects: Query<&GlobalTransform>,
    mut roots: Query<(Entity, &mut Node, &mut UiTransform), With<MarkerRoot>>,
    mut tops: Local<HashMap<String, (f32, f32, f32)>>,
    mut shown: Local<Option<(bool, UVec2, u32)>>,
    real: Res<Time<Real>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        *shown = None;
    }
    let clear =
        |commands: &mut Commands,
         roots: &mut Query<(Entity, &mut Node, &mut UiTransform), With<MarkerRoot>>| {
            for (e, _, _) in roots.iter() {
                commands.entity(e).try_despawn();
            }
        };
    let (Some(facts), Some(e)) = (current.1.as_ref(), current.0.entity()) else {
        if shown.take().is_some() {
            clear(&mut commands, &mut roots);
        }
        return;
    };
    let (Ok(win), Ok((cam, cam_gt, gc)), Ok(gt)) = (window.single(), cams.single(), objects.get(e))
    else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let (top, desc, icon) = *tops.entry(facts.record.clone()).or_insert_with(|| {
        let rec = data.db.get(&facts.record);
        let top = rec
            .as_ref()
            .and_then(|r| r.str("mesh").map(str::to_owned))
            .and_then(|p| data.vfs.read(&p).ok())
            .and_then(|b| gd_formats::msh::Mesh::parse(&b).ok())
            .and_then(|m| m.bounding_box)
            .map_or(2.0, |(_, max)| max[1]);
        let scale = rec
            .as_ref()
            .and_then(|r| r.f32("scale"))
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        let f = |k: &str| rec.as_ref().and_then(|r| r.i32(k)).unwrap_or(0) as f32;
        (top * scale, f("worldDescOffset"), f("interactIconPosition"))
    });
    let Ok(p) = cam.world_to_viewport(cam_gt, gt.translation() + Vec3::Y * (top + desc)) else {
        return;
    };
    let span = (gc.distance_max - gc.distance_min).max(1e-3);
    let zoom = 1.2 - (gc.distance - gc.distance_min) / span * 0.4;
    let p = (p * win.scale_factor()).floor();
    // `GetMachineTime` (ms).
    let ms = real.elapsed_secs() * 1000.0;
    let at = Vec2::new(
        p.x,
        p.y - (icon + facts.lift) * zoom + (ms * 0.004).sin() * 5.0 * zoom,
    );
    let inv = 1.0 / win.scale_factor();
    // The zoom eases through the root's `UiTransform`, so it is not part of the rebuild key.
    let s = assets.scale(size);
    let key = (facts.enemy, size, s.to_bits());
    if *shown == Some(key)
        && let Ok((_, mut node, mut tf)) = roots.single_mut()
    {
        node.left = Val::Px(at.x * inv);
        node.top = Val::Px(at.y * inv);
        tf.scale = Vec2::splat(zoom);
        return;
    }
    clear(&mut commands, &mut roots);
    *shown = Some(key);
    let root = commands
        .spawn((
            MarkerRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x * inv),
                top: Val::Px(at.y * inv),
                ..default()
            },
            UiTransform::from_scale(Vec2::splat(zoom)),
            GlobalZIndex(-1),
        ))
        .id();
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        inv,
        Vec2::new(size.x as f32, size.y as f32),
    );
    let rec = if facts.enemy {
        ENEMY_BITMAP
    } else {
        FRIEND_BITMAP
    };
    if let Some(Widget::Bitmap { bitmap, .. }) = ui.widget(rec)
        && let Some((h, sz)) = ui.image(&bitmap)
    {
        let (w, hh) = (sz.x as f32, sz.y as f32);
        ui.quad(
            h,
            URect::new(-w * 0.5, -hh * 0.5, w, hh),
            None,
            Color::WHITE,
        );
    }
}
