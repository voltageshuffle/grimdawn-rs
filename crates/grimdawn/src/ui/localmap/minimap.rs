use super::*;

/// The root of the minimap's node tree: the map quad, the frame, the [`MiniQuad`] pool and the [`MiniLabel`]
/// (module doc, PORT: built once per layout, changed in place).
#[derive(Component)]
pub(super) struct MiniRoot;

/// Pool node `n` of the minimap: it draws the frame's `n`-th [`Quad`] (the icons in draw order, then the north
/// marker) and is `Display::None` while the frame has fewer.
#[derive(Component)]
pub(super) struct MiniQuad(usize);

/// The holder of the hovered icon's name on the minimap: the name its lines were built for and their width (px).
#[derive(Component)]
pub(super) struct MiniLabel {
    name: String,
    width: f32,
}

/// One quad of the minimap as this frame draws it: an icon or the north marker.
pub(super) struct Quad {
    image: Handle<Image>,
    rect: URect,
    /// Clockwise.
    angle: f32,
    alpha: f32,
}

impl Quad {
    /// `size` px centred on `c`, its corner on a whole pixel (as [`icon_node`]).
    fn centred(image: Handle<Image>, c: Vec2, size: Vec2, angle: f32, alpha: f32) -> Self {
        Self {
            image,
            rect: URect::new(
                (c.x - size.x * 0.5).round(),
                (c.y - size.y * 0.5).round(),
                size.x,
                size.y,
            ),
            angle,
            alpha,
        }
    }

    fn color(&self) -> Color {
        Color::WHITE.with_alpha(self.alpha)
    }

    fn turn(&self) -> UiTransform {
        UiTransform::from_rotation(Rot2::radians(self.angle))
    }
}

/// The minimap's live nodes.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct MiniNodes<'w, 's> {
    roots: Query<'w, 's, Entity, With<MiniRoot>>,
    quads: Query<
        'w,
        's,
        (
            &'static MiniQuad,
            &'static mut Node,
            &'static mut ImageNode,
            &'static mut UiTransform,
        ),
    >,
    labels: Query<'w, 's, (Entity, &'static MiniLabel, &'static mut Node), Without<MiniQuad>>,
}

/// What the minimap's tree was built for: the window (px), the physical → logical factor's bits, the map image,
/// the bits of the two alphas ([`map_alphas`]), the compass mode, the HUD style (`compass_window` reads
/// `hud::master(classic)`) and the UI scale's bits. Layout inputs only: nothing here changes while the player moves.
pub(super) type MiniLayout = (UVec2, u32, AssetId<Image>, [u32; 2], u8, bool, u32);

/// The minimap in one compass mode ([`gd_game::compass::CompassMode`], [`compass_geometry`]).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CompassGeometry {
    /// The map's screen rect (px).
    rect: URect,
    /// The view's zoom (m × 3 over the map's height).
    zoom: f32,
    /// The frame bitmap and whether it is centred on the map (else its top-left corner is the map's).
    frame: (&'static str, bool),
    /// The map is drawn in the HUD table's floating alphas ([`map_alphas`]); otherwise in 1.
    floating: bool,
}

/// The minimap per compass mode on a `w` px wide screen at UI scale `s`; `compass` is the top-left corner of the
/// HUD table's `hudCompass` window (`hudwindow_compass.dbr`, its loaded screen rect).
/// `UIPlayerHud::ToggleCompass` (positions right-aligned: x = W − value), the HUD draw `UIPlayerHud::Render`
/// (frame per mode: 0 `hud_compasssmall`, 1 `hud_compasslarge`, 2 `hud_compassclosed`, 3 `hud_compassfloating`,
/// 4 `hud_compassfloatinglarge`, drawn at the map's corner; the two floating ones centred on it by
/// `UIMinimap::SetSize`) and the widget's update `DetailMapWidget::Update` (zoom 30 in mode 4, else 20):
/// - mode 3: a 160·s px map at (W − (160·s + 20), 50·s), mode 4: 246·s px at (W − (246·s + 25), 50·s), both in
///   the floating alphas;
/// - with (X, Y) the window's corner, the five values 110, 160, 246, X + 5 and Y + 9 are each scaled by s (the
///   window's already-scaled corner is scaled again): mode 1 a 160·s map at (W − (s(X + 5) + 160·s), s(Y + 9));
///   mode 0 a 110·s map at (W − (s(X + 5) + 12 + 110·s), 50·s + s(Y + 9) − 8); both in alpha 1;
/// - mode 2: the map is closed (size 0; `hud_compassclosed.tex` is fully transparent): `None`.
pub(super) fn compass_geometry(mode: u8, w: f32, s: f32, compass: Vec2) -> Option<CompassGeometry> {
    let (x5, y9) = (s * (compass.x + 5.0), s * (compass.y + 9.0));
    let at = |x: f32, y: f32, side: f32| {
        let side = (side * s).round();
        URect::new((w - (x + side)).round(), y.round(), side, side)
    };
    Some(match mode {
        2 => return None,
        0 => CompassGeometry {
            rect: at(x5 + 12.0, (160.0 - 110.0) * s + y9 - 8.0, 110.0),
            zoom: 20.0,
            frame: ("ui/hud/hud_compasssmall.tex", false),
            floating: false,
        },
        1 => CompassGeometry {
            rect: at(x5, y9, 160.0),
            zoom: 20.0,
            frame: ("ui/hud/hud_compasslarge.tex", false),
            floating: false,
        },
        4 => CompassGeometry {
            rect: at(25.0, 50.0 * s, 246.0),
            zoom: 30.0,
            frame: ("ui/hud/hud_compassfloatinglarge.tex", true),
            floating: true,
        },
        // 3 (any other value is left unplaced by the engine; the button's next press makes it 2).
        _ => CompassGeometry {
            rect: at(20.0, 50.0 * s, 160.0),
            zoom: 20.0,
            frame: ("ui/hud/hud_compassfloating.tex", true),
            floating: true,
        },
    })
}

/// The top-left corner of the HUD table's `hudCompass` window ([`compass_geometry`]), (0, 0) without one.
pub(super) fn compass_window(
    data: &GameData,
    options: &gd_game_core::options::GameOptions,
    w: f32,
    h: f32,
    s: f32,
) -> Vec2 {
    let rec = data
        .db
        .get(gd_ui_kit::master(gd_ui_kit::classic(Some(options))))
        .and_then(|t| t.str("hudCompass").and_then(|p| data.db.get(p)));
    rec.map_or(Vec2::ZERO, |r| {
        let rc = WindowDef::from_record(r).screen_rect(w, h, s);
        Vec2::new(rc.x, rc.y)
    })
}

/// The HUD minimap (module doc) in the compass mode's geometry ([`compass_geometry`]).
///
/// Every frame, as the widget's update and draw do (module doc): the map image is composited again when its view
/// or the explored ground changed, the icons are gathered and the tree's quads take their places. The tree itself
/// ([`MiniRoot`]) is built once per [`MiniLayout`]; nothing is despawned or spawned because something moved (a
/// pool node is added when a frame has more icons than any before it, the name when the hovered icon changes).
#[allow(clippy::too_many_arguments)]
pub(super) fn minimap(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    world: Option<Res<gd_world::StreamedMap>>,
    fog: Option<Res<gd_game::fog::FogOfWar>>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    (cams, player, mut src): Scene,
    pause: Res<gd_game_core::pause::GamePause>,
    mut nodes: MiniNodes,
    panels: Res<gd_ui::UiPanels>,
    mut tiles: ResMut<MapTiles>,
    mut assets: gd_ui_kit::UiAssets,
    (mut map, mut built, mut fresh, options, compass): (
        // The map image's cache and the offset / image the shown disc was cut for.
        Local<(Option<MapCache>, Option<(UVec2, AssetId<Image>)>)>,
        Local<Option<MiniLayout>>,
        gd_game_core::session::NewSession,
        Res<gd_game_core::options::GameOptions>,
        // Option: created once the player is in (`gd_game::compass::init_compass`); mode 3 until then.
        Option<Res<gd_game::compass::CompassMode>>,
    ),
) {
    if fresh.begun() {
        *map = Default::default();
        *built = None;
    }
    let mode = compass.map_or(gd_game::compass::NEW_CHARACTER_MODE, |c| c.0);
    let (Ok(win), Some(world), Ok((cam_t, cam))) = (window.single(), world, cams.single()) else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 {
        return;
    }
    let s = assets.scale(size);
    let (w, h) = (size.x as f32, size.y as f32);
    let geometry = compass_geometry(mode, w, s, compass_window(&data, &options, w, h, s));
    // Hidden under the full-screen devotion window (real reference shot), and closed in compass mode 2.
    let Some(geometry) = geometry.filter(|_| !panels.devotion) else {
        gd_ui::window::despawn_all(&mut commands, &nodes.roots);
        *built = None;
        return;
    };
    let rect = geometry.rect;
    let side = rect.w;
    let a = map_axes(cam_t, &options);
    let centre = Vec2::new(cam.target.x, cam.target.z);
    let mpp = geometry.zoom * 3.0 / side;
    let view = view_of(centre, a, mpp, side, side);
    // The map image: cut from the cache when the view moved a whole pixel, or the cache or the image changed.
    let px = UVec2::splat(side as u32);
    let target = MapTiles::target(&mut tiles.mini, &mut assets.images, px);
    let story = rt.as_ref().map(|r| r.story.borrow());
    let dungeon = story
        .as_ref()
        .and_then(|s| s.endless.dungeon.as_ref().map(|d| d.id));
    let in_dungeon = |i: usize| {
        story
            .as_ref()
            .is_some_and(|s| s.endless.in_dungeon(i, false))
    };
    let (cache, cut) = &mut *map;
    let (o, drawn) = MapCache::update(
        cache,
        &mut MapSource {
            tiles: &mut tiles,
            map: &world.map,
            fog: fog.as_deref(),
            fog_new: fog.as_ref().is_some_and(|f| f.is_added()),
            minimap: true,
            in_dungeon: &in_dungeon,
            dungeon,
        },
        &view,
        px,
        MAP_MARGIN,
    );
    if (drawn || *cut != Some((o, target.id())))
        && let Some(c) = cache.as_ref()
        && let Some(mut img) = assets.images.get_mut(&target)
    {
        *cut = Some((o, target.id()));
        img.data = Some(c.disc(o, px.x));
    }
    // Icons and their screen points (engine clamping), on the view the image is cut at (whole pixels) so they step
    // with the map under them.
    let shown = cache.as_ref().map_or(view, |c| c.shown(o));
    let pl = player
        .single()
        .ok()
        .map(|t| (t.translation, t.rotation * Vec3::Z));
    let icons = gather(
        &data,
        &assets.fonts,
        story.as_deref(),
        pl,
        &mut src,
        centre,
        // The disc's radius: `zoom × 3` m across the widget.
        geometry.zoom * 1.5 + GATHER_MARGIN,
    );
    let r = side * 0.5;
    let mut placed: Vec<(Vec2, MapIcon, f32)> = Vec::new();
    for i in icons {
        let sp = shown.screen([i.at.x, i.at.z]);
        let mut p = (Vec2::new(sp[0], sp[1]) - Vec2::splat(r)).round();
        if p.length() > r - 2.0 {
            // NPCs, food and Custom quads are not pinned (bits 2, 5, 14).
            if matches!(i.kind, 2 | 5 | 14) {
                continue;
            }
            p = p.normalize() * (r - 2.0);
        }
        let angle = i.facing.filter(|_| turns(i.kind)).map_or(0.0, |f| {
            let q = shown.screen([i.at.x + f.x, i.at.z + f.z]);
            angle_of(Vec2::new(q[0], q[1]) - Vec2::new(sp[0], sp[1]))
        });
        // A Custom quad's offset: px × widget width / 120 (`DetailMapWidget::RenderMinimapIcons`).
        let off = i.screen_offset(angle, side / 120.0);
        placed.push((p + off + Vec2::new(rect.x + r, rect.y + r), i, angle));
    }
    // Custom quads go under the other icons (their own pass).
    placed.sort_by_key(|(_, i, _)| i.kind != 14);
    // The game menu / Options holds the cursor: no icon name under it.
    let cursor = gd_ui_kit::cursor_px(&hooks, win).filter(|_| !pause.menu);
    let isz = (16.0 * s).round();
    let hover = cursor
        .and_then(|c| {
            placed.iter().rfind(|(p, i, _)| {
                i.kind != 14 && !i.name.is_empty() && (c - *p).length() < isz * 0.5
            })
        })
        .map(|(p, i, _)| (*p, i.name.as_str()));
    // The tree: kept while its layout holds.
    let inv = 1.0 / win.scale_factor();
    let (map_a, marker_a) = if geometry.floating {
        map_alphas(&data, &options)
    } else {
        (1.0, 1.0)
    };
    let layout: MiniLayout = (
        size,
        inv.to_bits(),
        target.id(),
        [map_a.to_bits(), marker_a.to_bits()],
        mode,
        assets.classic_hud(),
        s.to_bits(),
    );
    let mut live = nodes.roots.iter();
    let kept = match (live.next(), live.next()) {
        (Some(root), None) if built.as_ref() == Some(&layout) => Some(root),
        _ => None,
    };
    let root = match kept {
        Some(root) => root,
        None => {
            gd_ui::window::despawn_all(&mut commands, &nodes.roots);
            gd_ui_kit::spawn_hud_overlay(&mut commands, MiniRoot)
        }
    };
    *built = Some(layout);
    let mut ui = assets.ui(&mut commands, &data, root, s, inv, size.as_vec2());
    if kept.is_none() {
        ui.quad_stretch(target, rect, Color::WHITE.with_alpha(map_a));
        let (frame, centred) = geometry.frame;
        if let Some((img, fs)) = ui.image(frame) {
            let (fw, fh) = (ui.sc(fs.x as f32), ui.sc(fs.y as f32));
            let (dx, dy) = if centred {
                (((rect.w - fw) * 0.5).trunc(), ((rect.h - fh) * 0.5).trunc())
            } else {
                (0.0, 0.0)
            };
            ui.quad(
                img,
                URect::new(rect.x + dx, rect.y + dy, fw, fh),
                None,
                Color::WHITE,
            );
        }
    }
    // This frame's quads: the icons, then the north marker on the circle.
    let mut quads: Vec<Quad> = Vec::with_capacity(placed.len() + 1);
    for (p, i, angle) in &placed {
        let Some((img, ts)) = icon_image(&mut ui, i, false, false) else {
            continue;
        };
        let sz = match i.kind {
            17 => Vec2::splat((22.0 * s).round()),
            // Custom: texture px × scale × widget width / 120.
            14 => ts.as_vec2() * i.scale * side / 120.0,
            _ => Vec2::splat(isz),
        };
        quads.push(Quad::centred(img, *p, sz, *angle, map_a));
    }
    let n = Vec3::new(-1.0, 0.0, -1.0).normalize();
    let (c0, c1) = (
        view.screen([centre.x, centre.y]),
        view.screen([centre.x + n.x, centre.y + n.z]),
    );
    let dir = (Vec2::new(c1[0], c1[1]) - Vec2::new(c0[0], c0[1])).normalize_or_zero();
    if let Some((img, ns)) = ui.image("ui/mapaerial/mapsymbol_north.tex") {
        let sz = Vec2::new(ui.sc(ns.x as f32), ui.sc(ns.y as f32));
        let c = Vec2::new(rect.x + r, rect.y + r) + dir * r;
        quads.push(Quad::centred(img, c, sz, angle_of(dir), marker_a));
    }
    // The pool takes them in place (a new tree has no pool yet: the nodes still alive are the old tree's).
    let mut pool = 0;
    if kept.is_some() {
        for (slot, mut node, mut image, mut turn) in &mut nodes.quads {
            pool = pool.max(slot.0 + 1);
            let Some(q) = quads.get(slot.0) else {
                if node.display != Display::None {
                    node.display = Display::None;
                }
                continue;
            };
            node.set_if_neq(ui.node(q.rect));
            if image.image != q.image {
                image.image = q.image.clone();
            }
            if image.color != q.color() {
                image.color = q.color();
            }
            turn.set_if_neq(q.turn());
        }
    }
    for (n, q) in quads.iter().enumerate().skip(pool) {
        let e = ui.quad_stretch(q.image.clone(), q.rect, q.color());
        ui.commands.entity(e).insert((MiniQuad(n), q.turn()));
    }
    // The hovered icon's name, left of it: its holder follows the icon; another name is another holder. The holder
    // is as large as the screen so that its lines never wrap.
    let label_at = |p: Vec2, width: f32| {
        URect::new(
            (p.x - isz * 0.5 - width - 3.0).round(),
            (p.y - isz * 0.5 + 5.0).round(),
            size.x as f32,
            size.y as f32,
        )
    };
    let mut shown = false;
    for (e, label, mut node) in &mut nodes.labels {
        match hover.filter(|(_, name)| kept.is_some() && !shown && *name == label.name) {
            Some((p, _)) => {
                shown = true;
                node.set_if_neq(ui.node(label_at(p, label.width)));
            }
            None => {
                ui.commands.entity(e).try_despawn();
            }
        }
    }
    if let (false, Some((p, name))) = (shown, hover) {
        let width = name
            .lines()
            .map(|l| ui.text_width(NAME_STYLE, l))
            .fold(0.0, f32::max);
        let lh = ui.line_height(NAME_STYLE);
        let node = ui.node(label_at(p, width));
        let holder = ui
            .commands
            .spawn((
                MiniLabel {
                    name: name.to_owned(),
                    width,
                },
                node,
                ZIndex(1),
            ))
            .id();
        ui.commands.entity(root).add_child(holder);
        ui.root = holder;
        for (k, l) in name.lines().enumerate() {
            ui.text_line(NAME_STYLE, Vec2::new(0.0, (k as f32 * lh).round()), l, 1.0);
        }
    }
}
