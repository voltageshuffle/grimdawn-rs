use super::*;

/// One map icon: world point, nugget type (module doc), name, facing, the
/// nugget's own bitmap (types 14 / 19), its scale (type 14: 0.75) and a
/// Custom quad's offset (quest `areaOffsetX` / `Y`: px along / across its
/// facing).
#[derive(Clone, Debug, PartialEq)]
pub struct MapIcon {
    pub at: Vec3,
    pub kind: u8,
    pub name: String,
    pub facing: Option<Vec3>,
    pub bitmap: Option<String>,
    pub scale: f32,
    pub offset: Vec2,
}

impl MapIcon {
    pub(super) fn new(at: Vec3, kind: u8, name: String) -> Self {
        Self {
            at,
            kind,
            name,
            facing: None,
            bitmap: None,
            scale: 1.0,
            offset: Vec2::ZERO,
        }
    }

    /// A Custom quad's screen offset: `offset` × `k` px along the screen
    /// direction of its facing (`angle`, [`angle_of`]) and across it
    /// (sub_140177010 / sub_1401786D0: x·d + y·(−d.y, d.x)).
    pub(super) fn screen_offset(&self, angle: f32, k: f32) -> Vec2 {
        let d = Vec2::new(angle.sin(), -angle.cos());
        (d * self.offset.x + Vec2::new(-d.y, d.x) * self.offset.y) * k
    }
}

/// A level's `AreaOfInterest` (spawned with the level, `world.rs`).
#[derive(Component)]
pub struct MapPoint {
    pub record: String,
}

/// `Actor::Load`'s `mapNuggetType` (Engine.dll sub_1800239D0; `None` / unknown: no icon).
pub(super) fn nugget_type(name: &str) -> Option<u8> {
    Some(match name {
        "Hero" => 0,
        "Party" => 1,
        "NPC" => 2,
        "Teleport" => 3,
        "Respawn" => 4,
        "Shrine" => 5,
        "AreaOfInterest" => 6,
        "Merchant" => 7,
        "Quest" => 8,
        "Marker" => 10,
        "Enchanter" => 11,
        "Grave" => 12,
        "Caravan" => 13,
        "Custom" => 14,
        "Crafter" => 15,
        "Boss" => 17,
        "Transmuter" => 18,
        "Regular" => 19,
        "EndlessDungeonExit" => 20,
        _ => return None,
    })
}

/// `Actor::AppendDetailMapData` (Engine.dll 0x18002a730): the record's
/// `mapNuggetType` with `mapNuggetCustom` as its bitmap; a Custom one needs
/// the bitmap and draws at 0.75.
pub(super) fn actor_icon(
    rec: &gd_formats::arz::Record,
    at: Vec3,
    facing: Vec3,
    name: String,
) -> Option<MapIcon> {
    let kind = nugget_type(rec.str("mapNuggetType")?)?;
    let bitmap = rec
        .str("mapNuggetCustom")
        .filter(|b| !b.is_empty())
        .map(str::to_owned);
    if kind == 14 && bitmap.is_none() {
        return None;
    }
    Some(MapIcon {
        facing: Some(facing),
        bitmap,
        scale: if kind == 14 { 0.75 } else { 1.0 },
        ..MapIcon::new(at, kind, name)
    })
}

/// A quest marker's area quad (sub_180428780 load, sub_180428A00 push): the
/// record's `areaBitmap` as a Custom nugget (scale 1) at the entity, facing
/// `areaRotation` degrees (world (cos r, 0, sin r)) when `areaRotate` is set,
/// else the entity's facing, offset by `areaOffsetX` / `areaOffsetY` (ints).
/// Pushed before the star, so it draws under it. No stock record sets
/// `areaBitmap`.
pub(super) fn area_icon(rec: &gd_formats::arz::Record, at: Vec3, facing: Vec3) -> Option<MapIcon> {
    let bitmap = rec.str("areaBitmap").filter(|b| !b.is_empty())?.to_owned();
    let facing = if rec.bool("areaRotate") {
        let r = rec.f32("areaRotation").unwrap_or(0.0).to_radians();
        Vec3::new(r.cos(), 0.0, r.sin())
    } else {
        facing
    };
    let offset = Vec2::new(
        rec.i32("areaOffsetX").unwrap_or(0) as f32,
        rec.i32("areaOffsetY").unwrap_or(0) as f32,
    );
    Some(MapIcon {
        facing: Some(facing),
        bitmap: Some(bitmap),
        offset,
        ..MapIcon::new(at, 14, String::new())
    })
}

/// The bitmap of a nugget type (`local`: the local map's variant).
pub(super) fn icon_texture(kind: u8, local: bool) -> Option<&'static str> {
    Some(match kind {
        0 if local => "ui/mapaerial/symbols/mapsymbol_herodirectional.tex",
        0 => "ui/mapaerial/symbols/mapsymbol_herodirectionalover.tex",
        1 => "ui/mapaerial/symbols/mapsymbol_party.tex",
        2 => "ui/mapaerial/symbols/mapsymbol_npc.tex",
        3 => "ui/riftgatemap/icon_riftgatedown.tex",
        4 => "ui/mapaerial/symbols/mapsymbol_respawn.tex",
        5 => "ui/mapaerial/symbols/mapsymbol_meal.tex",
        6 => "ui/mapaerial/symbols/mapsymbol_aoi.tex",
        7 => "ui/mapaerial/symbols/mapsymbol_vendor.tex",
        8 => "ui/mapaerial/symbols/mapsymbol_mission.tex",
        10 => "ui/mapaerial/symbols/mapsymbol_witch.tex",
        11 => "ui/mapaerial/symbols/mapsymbol_inventor.tex",
        12 => "ui/mapaerial/symbols/mapsymbol_grave.tex",
        13 => "ui/mapaerial/symbols/mapsymbol_caravan.tex",
        15 => "ui/mapaerial/symbols/mapsymbol_smith.tex",
        16 => "ui/mapaerial/symbols/mapsymbol_shrine.tex",
        17 => "ui/mapaerial/symbols/mapsymbol_boss.tex",
        18 => "ui/mapaerial/symbols/mapsymbol_illusionist.tex",
        20 => "ui/mapaerial/symbols/mapsymbol_exit.tex",
        _ => return None,
    })
}

/// A character record's own nugget type (`AppendDetailMapData` by class).
pub(super) fn npc_kind(rec: &gd_formats::arz::Record) -> Option<u8> {
    Some(match rec.class.as_str() {
        "NpcMerchant" | "NpcItemAscension" => 7,
        "NpcCrafter" => 15,
        "NpcEnchanter" => 11,
        "NpcTransmuter" => 18,
        "NpcCaravan" => 13,
        "NpcSkillReallocator" => 10,
        "Npc" if rec.bool("IncludeInMap") => 2,
        _ => return None,
    })
}

/// The quest names of the record's map marker while one of its tasks is in
/// progress (Game.dll sub_180428780 load, sub_180428A00 test).
pub(super) fn quest_marker(
    rec: &gd_formats::arz::Record,
    story: &gd_sim::story::Story,
    at: Vec3,
    player: Vec3,
) -> Option<String> {
    let range = rec.f32("markerRange").unwrap_or(0.0);
    if range > 0.0 && at.distance_squared(player) > range * range {
        return None;
    }
    let log = &story.quests;
    let mut names: Vec<String> = Vec::new();
    for n in 1..=10 {
        let Some(file) = rec.str(&format!("questFile{n}")).filter(|f| !f.is_empty()) else {
            break;
        };
        let uid = rec.i32(&format!("taskUid{n}")).unwrap_or(0) as u32;
        let Some(qi) = log.defs.by_path(file) else {
            continue;
        };
        let Some(ti) = log.task_index(qi, uid) else {
            continue;
        };
        if log.task_state(qi, ti) == gd_sim::story::quest::STATE_IN_PROGRESS {
            let name = gd_formats::text::strip_markup(log.defs.list[qi].quest.name());
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    (!names.is_empty()).then(|| names.join("\n"))
}

/// A record's localised `description`.
pub(super) fn described(rec: &gd_formats::arz::Record, fonts: &FontCache) -> String {
    fonts.tag(rec.str("description").unwrap_or("")).to_owned()
}

/// The rollover name of a character (`Npc::GetRolloverDescription`).
pub(super) fn npc_name(rec: &gd_formats::arz::Record, fonts: &FontCache) -> String {
    let base = fonts.tag(rec.str("description").unwrap_or("")).to_owned();
    match rec.str("professionTag").filter(|p| !p.is_empty()) {
        Some(p) => format!("{base} - {}", fonts.tag(p)),
        None => base,
    }
}

pub(super) type Friends<'w, 's> = Query<
    'w,
    's,
    (
        &'static GlobalTransform,
        &'static gd_render::character::Character,
        Option<&'static gd_game::story::StoryActor>,
        Option<&'static Visibility>,
    ),
    (
        Without<gd_game::monsters::Monster>,
        Without<crate::player::Player>,
        Without<crate::combat::Dead>,
    ),
>;

/// Everything that appends map nuggets (module doc).
#[derive(bevy::ecs::system::SystemParam)]
pub struct MapSources<'w, 's> {
    pub(super) friends: Friends<'w, 's>,
    pub(super) gates: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static gd_game::riftgates::Waypoint,
        ),
    >,
    pub(super) shrines:
        Query<'w, 's, (&'static GlobalTransform, &'static gd_game::devotion::Shrine)>,
    pub(super) monsters: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static gd_game::monsters::Monster,
            Option<&'static Visibility>,
            Has<crate::combat::Allied>,
        ),
        Without<crate::combat::Dead>,
    >,
    pub(super) breakables:
        Query<'w, 's, (&'static GlobalTransform, &'static gd_game::world::Breakable)>,
    pub(super) chests: Query<'w, 's, (&'static GlobalTransform, &'static gd_game::chests::Chest)>,
    pub(super) endless_shrines: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static gd_game::endless::EndlessShrine,
        ),
    >,
    pub(super) exit_gates:
        Query<'w, 's, &'static GlobalTransform, With<gd_game::endless::EndlessGate>>,
    pub(super) items: Query<'w, 's, (&'static GlobalTransform, &'static gd_game::loot::GroundItem)>,
    pub(super) graves: Query<'w, 's, &'static GlobalTransform, With<gd_game::death::Gravestone>>,
    pub(super) points: Query<'w, 's, (&'static GlobalTransform, &'static MapPoint)>,
    pub(super) devotion: Option<Res<'w, gd_game::devotion::DevotionState>>,
    pub(super) inv: Option<Res<'w, crate::inventory::Inventory>>,
    pub(super) filter: Option<Res<'w, gd_game::loot_filter::LootFilter>>,
    pub(super) loadout:
        Query<'w, 's, &'static gd_game::player::Loadout, With<crate::player::Player>>,
    pub(super) values: Res<'w, crate::ui::HudValues>,
}

/// The quest marker of an entity (`sub_180428A00`: its area quad, then the
/// star), else its own icon.
pub(super) fn marker_or(
    rec: &gd_formats::arz::Record,
    story: Option<&gd_sim::story::Story>,
    at: Vec3,
    facing: Vec3,
    player: Vec3,
    own: impl FnOnce() -> Option<MapIcon>,
) -> Vec<MapIcon> {
    if let Some(quests) = story.and_then(|st| quest_marker(rec, st, at, player)) {
        return area_icon(rec, at, facing)
            .into_iter()
            .chain([MapIcon::new(at, 8, quests)])
            .collect();
    }
    own().into_iter().collect()
}

/// Gathers the icons around `centre` within `radius` metres.
#[allow(clippy::too_many_arguments)]
pub(super) fn gather(
    data: &GameData,
    fonts: &FontCache,
    story: Option<&gd_sim::story::Story>,
    player: Option<(Vec3, Vec3)>,
    src: &mut MapSources,
    centre: Vec2,
    radius: f32,
) -> Vec<MapIcon> {
    let near = |p: Vec3| (Vec2::new(p.x, p.z) - centre).abs().max_element() <= radius;
    let ppos = player.map_or(Vec3::ZERO, |p| p.0);
    let mut out = Vec::new();
    let forward = |t: &GlobalTransform| t.rotation() * Vec3::Z;
    // The main player's grave (`GameEngine::GetDetailMapData`).
    for t in &src.graves {
        let p = t.translation();
        if near(p) {
            out.push(MapIcon::new(
                p,
                12,
                fonts
                    .tag("tagGrave01")
                    .replace("{%t0}", &src.values.name)
                    .replace("%t0", &src.values.name),
            ));
        }
    }
    // Areas of interest: a quest marker, else (unless `questOnly`) type 6;
    // static scenery: its `mapNuggetType`.
    for (t, m) in &src.points {
        let p = t.translation();
        let Some(rec) = data.db.get(&m.record).filter(|_| near(p)) else {
            continue;
        };
        if rec.class == "AreaOfInterest" {
            let name = fonts
                .tag(rec.str("AreaDescription").unwrap_or(""))
                .to_owned();
            out.extend(marker_or(rec, story, p, forward(t), ppos, || {
                (!rec.bool("questOnly")).then(|| MapIcon::new(p, 6, name))
            }));
        } else {
            out.extend(actor_icon(rec, p, forward(t), described(rec, fonts)));
        }
    }
    // Monsters and the player's pets (`Monster::AppendDetailMapData`, the
    // campaign branch: alive, not hiding). Game.dll @0x180375810 (the dungeon branch, a monster in
    // the running Shattered Realm dungeon): its own icon (no quest marker) until the generator reveals the
    // monsters (`gd_sim::endless::Dungeon::reveal_monsters`); then a live foe is a star (type 8) or, with a
    // classification of Boss and up, the boss icon (17), named by its description, and others get none.
    // PORT: the dungeon's monsters are those of its activated proxies (the engine: in any of its regions).
    let dungeon = story.and_then(|s| s.endless.dungeon.as_ref().map(|d| (s, d)));
    let reveal = dungeon.is_some_and(|(_, d)| d.reveal_monsters());
    for (t, m, vis, allied) in &src.monsters {
        let p = t.translation();
        if !near(p) || vis.is_some_and(|v| *v == Visibility::Hidden) {
            continue;
        }
        let Some(rec) = data.db.get(&m.record) else {
            continue;
        };
        let in_dungeon = dungeon.is_some_and(|(s, _)| {
            s.endless
                .activated
                .iter()
                .any(|(_, key)| *key == (m.slot.0, m.slot.1))
        });
        if !in_dungeon {
            out.extend(marker_or(rec, story, p, forward(t), ppos, || {
                actor_icon(rec, p, forward(t), described(rec, fonts))
            }));
        } else if !reveal {
            out.extend(actor_icon(rec, p, forward(t), described(rec, fonts)));
        } else if !allied {
            let boss = gd_sim::story::quest::monster_classification(
                rec.str("monsterClassification").unwrap_or(""),
            ) >= 3;
            out.push(MapIcon::new(
                p,
                if boss { 17 } else { 8 },
                described(rec, fonts),
            ));
        }
    }
    // Breakables (`Destructible`: unbroken; broken ones are gone here).
    for (t, b) in &src.breakables {
        let p = t.translation();
        let Some(rec) = data.db.get(&b.record).filter(|_| near(p)) else {
            continue;
        };
        out.extend(marker_or(rec, story, p, forward(t), ppos, || {
            actor_icon(rec, p, forward(t), described(rec, fonts))
        }));
    }
    // Closed chests (`FixedItemContainer`, state 1).
    for (t, c) in &src.chests {
        let p = t.translation();
        let Some(rec) = data.db.get(&c.record).filter(|_| near(p) && !c.opened) else {
            continue;
        };
        out.extend(marker_or(rec, story, p, forward(t), ppos, || {
            actor_icon(rec, p, forward(t), described(rec, fonts))
        }));
    }
    // The Shattered Realm's buff shrines: the record's own Custom nugget (`mapsymbol_shrine<colour>_up`) through
    // `Actor::AppendDetailMapData`. PORT: always drawn, used or not (whether `StaticShrine`'s type 16 or a
    // used state changes it for `EndlessBuffShrine` is not read).
    for (t, s) in &src.endless_shrines {
        let p = t.translation();
        if let Some(rec) = data.db.get(&s.record).filter(|_| near(p)) {
            out.extend(actor_icon(rec, p, forward(t), described(rec, fonts)));
        }
    }
    // The Shattered Realm's exit gates: the record's `mapsymbol_exit` (`FixedItemDungeonTeleport::AppendDetailMapData`
    // @0x180274CF0), named by its `textTag` ("Next Floor").
    if let Some(rec) = data.db.get(gd_game::endless::EXIT_GATE_RECORD) {
        let name = fonts.tag(rec.str("textTag").unwrap_or("")).to_owned();
        for t in &src.exit_gates {
            let p = t.translation();
            if near(p) {
                out.extend(actor_icon(rec, p, forward(t), name.clone()));
            }
        }
    }
    // Ground items (`Item::AppendDetailMapData`): a quest marker, else the
    // rarity's `<key>ItemMapNugget` (type 19) when the loot filter passes it.
    let info_rec = data.db.get("records/game/gameiteminfo.dbr");
    let loadout = src.loadout.iter().next();
    for (t, g) in &src.items {
        let p = t.translation();
        let (Some(item), true) = (&g.item, near(p)) else {
            continue;
        };
        let Some(rec) = data.db.get(&item.base_name) else {
            continue;
        };
        let Some(inv) = src.inv.as_deref() else {
            continue;
        };
        let Some(info) = inv.info(item) else { continue };
        let name = gd_sim::items::naming::item_name(&fonts.tags, &info.resolved);
        let bitmap = crate::ui::character::rarity_key(info.resolved.display_color_index())
            .and_then(|k| {
                info_rec
                    .as_ref()?
                    .str(&format!("{k}ItemMapNugget"))
                    .filter(|b| !b.is_empty())
                    .map(str::to_owned)
            });
        let passes = src.filter.as_ref().is_none_or(|f| {
            f.passes(
                inv,
                &data.db,
                item,
                gd_sim::items::loot_filter::Mode::Filter,
                loadout,
            )
        });
        out.extend(marker_or(rec, story, p, forward(t), ppos, || {
            bitmap.filter(|_| passes).map(|b| MapIcon {
                bitmap: Some(b),
                ..MapIcon::new(p, 19, name)
            })
        }));
    }
    for (t, w) in &src.gates {
        let p = t.translation();
        if near(p) {
            let name = data
                .db
                .get(&w.record)
                .and_then(|r| r.str("description").map(|d| fonts.tag(d).to_owned()))
                .unwrap_or_default();
            // A respawn point only with a name (StaticRespawner::AppendDetailMapData).
            if w.kind == WaypointKind::Riftgate || !name.is_empty() {
                out.push(MapIcon::new(
                    p,
                    if w.kind == WaypointKind::Riftgate {
                        3
                    } else {
                        4
                    },
                    name,
                ));
            }
        }
    }
    // Devotion shrines (16) and monster totems (`MonsterShrine`: type 19
    // with its bitmap until cleared); `hideMapIcon` hides either.
    for (t, s) in &src.shrines {
        let p = t.translation();
        let Some(rec) = data
            .db
            .get(&s.record)
            .filter(|r| near(p) && !r.bool("hideMapIcon"))
        else {
            continue;
        };
        let name = described(rec, fonts);
        if !s.totem {
            out.push(MapIcon::new(p, 16, name));
        } else if s.is_of_interest(src.devotion.as_deref())
            && let Some(b) = rec.str("mapNuggetCustom").filter(|b| !b.is_empty())
        {
            out.push(MapIcon {
                bitmap: Some(b.to_owned()),
                ..MapIcon::new(p, 19, name)
            });
        }
    }
    for (t, c, actor, vis) in &src.friends {
        let p = t.translation();
        if !near(p) || vis.is_some_and(|v| *v == Visibility::Hidden) {
            continue;
        }
        if let (Some(a), Some(st)) = (actor, story)
            && st.entities.get(a.0).is_some_and(|o| !o.in_world)
        {
            continue;
        }
        let Some(rec) = data.db.get(&c.record) else {
            continue;
        };
        if rec.class == "Npc"
            && let Some(st) = story
            && let Some(quests) = quest_marker(rec, st, p, ppos)
        {
            out.extend(area_icon(rec, p, forward(t)));
            out.push(MapIcon::new(p, 8, quests));
            continue;
        }
        if let Some(kind) = npc_kind(rec) {
            // NPCs turn with their facing (sub_140177010 cases 0–2).
            out.push(MapIcon {
                facing: (kind == 2).then(|| forward(t)),
                ..MapIcon::new(p, kind, npc_name(rec, fonts))
            });
        }
    }
    if let Some((p, f)) = player {
        out.push(MapIcon {
            facing: Some(f),
            ..MapIcon::new(p, 0, String::new())
        });
    }
    out
}
