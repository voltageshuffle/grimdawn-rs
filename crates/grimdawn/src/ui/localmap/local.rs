use super::*;
use gd_ui::state::map::{LEGEND, LocalLayout, MapWindow};

/// The root of the local map's tree, built per window size; its [`LocalLayer`]s are rebuilt each on its own
/// signature ([`LocalBuilt`]).
#[derive(Component)]
pub(super) struct LocalRoot;

/// A layer of [`LocalRoot`], in draw order: the map quad and the icons, the window (frame, title, legend, buttons),
/// the hovered icon's name.
#[derive(Component, Clone, Copy, PartialEq)]
pub(super) enum LocalLayer {
    Map,
    Chrome,
    Label,
}

/// The local map's quad: its source rect follows the view across the [`MapCache`] image in place.
#[derive(Component)]
pub(super) struct LocalMapQuad;

/// The local map's live nodes.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct LocalNodes<'w, 's> {
    roots: Query<'w, 's, Entity, With<LocalRoot>>,
    layers: Query<'w, 's, (Entity, &'static LocalLayer)>,
    quad: Query<'w, 's, &'static mut ImageNode, With<LocalMapQuad>>,
}

impl LocalNodes<'_, '_> {
    /// Replaces layer `kind` under `root` with an empty one (sibling order = draw order).
    fn relayer(&self, commands: &mut Commands, root: Entity, kind: LocalLayer) -> Entity {
        for (e, k) in &self.layers {
            if *k == kind {
                commands.entity(e).try_despawn();
            }
        }
        let e = commands
            .spawn((
                kind,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                ZIndex(kind as i32),
            ))
            .id();
        commands.entity(root).add_child(e);
        e
    }
}

/// What the local map's tree was built for, per part (`None`: not built), and its map image.
#[derive(Default)]
pub(super) struct LocalBuilt {
    cache: Option<MapCache>,
    /// [`LocalRoot`]: the window size.
    root: Option<UVec2>,
    /// [`LocalLayer::Map`]: the map image, the icons (px, type, angle × 100), the legend blinks.
    map: Option<(AssetId<Image>, Vec<(Vec2, u8, i32)>, [i32; 21])>,
    /// [`LocalLayer::Chrome`]: the area name, the hovered legend / window button, the UI scale.
    chrome: Option<(String, Option<usize>, Option<usize>, u32)>,
    /// [`LocalLayer::Label`]: the hovered icon's name and point.
    label: Option<Option<(String, Vec2)>>,
}

/// The local map window (module doc), rebuilt when its view or hover changes.
#[allow(clippy::too_many_arguments)]
pub(super) fn local_map(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    time: Res<Time<Real>>,
    world: Option<Res<gd_world::StreamedMap>>,
    fog: Option<Res<gd_game::fog::FogOfWar>>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    (rift, options, pause): (
        Res<RiftgateMap>,
        Res<gd_game_core::options::GameOptions>,
        Res<gd_game_core::pause::GamePause>,
    ),
    values: Res<gd_ui::HudValues>,
    (cams, player, mut src): Scene,
    mut nodes: LocalNodes,
    mut win_state: ResMut<MapWindow>,
    mut tiles: ResMut<MapTiles>,
    mut assets: gd_ui_kit::UiAssets,
    (mut built, mut fresh): (Local<LocalBuilt>, gd_game_core::session::NewSession),
) {
    if fresh.begun() {
        *built = Default::default();
    }
    let shown = win_state.local_shown(&rift);
    let (Ok(win), Some(world), Ok((cam_t, cam))) = (window.single(), world, cams.single()) else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if !shown || size.x == 0 {
        if built.root.is_some() {
            gd_ui::window::despawn_all(&mut commands, &nodes.roots);
            win_state.layout = LocalLayout::default();
        }
        // The map image stays: opening again shows it at once.
        *built = LocalBuilt {
            cache: built.cache.take(),
            ..Default::default()
        };
        return;
    }
    // Legend blinks run down (`LocalMap::Update`).
    let dt = time.delta_secs() * 1000.0;
    for b in win_state.blink.iter_mut() {
        *b = (*b - dt).max(0.0);
    }
    let blink_sig: [i32; 21] = win_state.blink.map(|b| (b * 0.02) as i32);
    // Zoom easing (0.06 m per ms).
    let step = time.delta_secs() * 1000.0 * 0.06;
    let (z, zt) = (win_state.zoom, win_state.zoom_to);
    win_state.zoom = if (z - zt).abs() <= step {
        zt
    } else {
        z + step * (zt - z).signum()
    };
    let Some(rec) = data.db.get(AERIAL) else {
        return;
    };
    let s = assets.scale(size);
    let wr = data
        .db
        .get(crate::ui::riftmap::WINDOW)
        .map(WindowDef::from_record)
        .unwrap_or_else(|| WindowDef::from_record(rec))
        .screen_rect(size.x as f32, size.y as f32, s);
    let num = |k: &str| rec.f32(k).unwrap_or(0.0);
    let sc = |v: f32| gd_formats::ui::round_px(v * s);
    let pane = URect::new(
        wr.x + sc(num("MapPaneX")),
        wr.y + sc(num("MapPaneY")),
        sc(num("MapPaneWidth")),
        sc(num("MapPaneHeight")),
    );
    let a = map_axes(cam_t, &options);
    let mpp = win_state.zoom * 3.0 / pane.h.max(1.0);
    let centre =
        Vec2::new(cam.target.x, cam.target.z) + a.right * win_state.pan.x + a.up * win_state.pan.y;
    let view = view_of(centre, a, mpp, pane.w, pane.h);
    let story = rt.as_ref().map(|r| r.story.borrow());
    let pl = player
        .single()
        .ok()
        .map(|t| (t.translation, t.rotation * Vec3::Z));
    // The view turns with the camera: the pane's half-diagonal covers it at any angle.
    let radius = 0.5 * pane.w.hypot(pane.h) * mpp + GATHER_MARGIN;
    let mut icons = gather(
        &data,
        &assets.fonts,
        story.as_deref(),
        pl,
        &mut src,
        centre,
        radius,
    );
    icons.retain(|i| i.kind == 0 || fog.as_ref().is_none_or(|f| !f.is_in_fog(&world.map, i.at)));
    let dungeon = story
        .as_ref()
        .and_then(|s| s.endless.dungeon.as_ref().map(|d| d.id));
    // The map image: the view scrolls across the cache; while the zoom eases (a new scale every frame) it is drawn
    // without a margin.
    let px = UVec2::new(pane.w as u32, pane.h as u32);
    let in_dungeon = |i: usize| {
        story
            .as_ref()
            .is_some_and(|s| s.endless.in_dungeon(i, false))
    };
    let (o, drawn) = MapCache::update(
        &mut built.cache,
        &mut MapSource {
            tiles: &mut tiles,
            map: &world.map,
            fog: fog.as_deref(),
            fog_new: fog.as_ref().is_some_and(|f| f.is_added()),
            minimap: false,
            in_dungeon: &in_dungeon,
            dungeon,
        },
        &view,
        px,
        if win_state.zoom == win_state.zoom_to {
            MAP_MARGIN
        } else {
            0.0
        },
    );
    let Some(cache) = built.cache.as_ref() else {
        return;
    };
    let target = MapTiles::target(&mut tiles.local, &mut assets.images, cache.size);
    if drawn && let Some(mut img) = assets.images.get_mut(&target) {
        img.data = Some(cache.rgba.clone());
    }
    // Icons are placed on the view the image is cut at (whole pixels), so they step with the map under them.
    let shown = cache.shown(o);
    let isz = (24.0 * s).round();
    let placed: Vec<(Vec2, MapIcon, f32)> = icons
        .into_iter()
        .filter_map(|i| {
            let sp = shown.screen([i.at.x, i.at.z]);
            let p = Vec2::new(sp[0], sp[1]).round();
            if p.x < 0.0 || p.y < 0.0 || p.x >= pane.w || p.y >= pane.h {
                return None;
            }
            let angle = i.facing.filter(|_| turns(i.kind)).map_or(0.0, |f| {
                let q = shown.screen([i.at.x + f.x, i.at.z + f.z]);
                angle_of(Vec2::new(q[0] - sp[0], q[1] - sp[1]))
            });
            // A Custom quad's offset: px × 0.3125 / zoom × 460, no UI
            // scale (`DetailMapWidget::RenderLocalMapIcons`).
            let off = i.screen_offset(angle, 0.3125 / win_state.zoom * 460.0);
            Some((p + off + Vec2::new(pane.x, pane.y), i, angle))
        })
        .collect();
    // The game menu / Options holds the cursor: no icon name under it.
    let cursor = gd_ui_kit::cursor_px(win).filter(|_| !pause.menu);
    let hover = cursor.and_then(|c| {
        placed.iter().rposition(|(p, i, _)| {
            i.kind != 14 && !i.name.is_empty() && (c - *p).length() < isz * 0.5
        })
    });
    let src_rect = Rect::new(
        o.x as f32,
        o.y as f32,
        (o.x + px.x) as f32,
        (o.y + px.y) as f32,
    );
    let root = match nodes.roots.iter().next() {
        Some(r) if built.root == Some(size) => r,
        _ => {
            gd_ui::window::despawn_all(&mut commands, &nodes.roots);
            built.root = Some(size);
            built.map = None;
            built.chrome = None;
            built.label = None;
            gd_ui_kit::spawn_root(&mut commands, LocalRoot, gd_ui::window::Z_WINDOW)
        }
    };
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        size.as_vec2(),
    );
    let map_sig = (
        target.id(),
        placed
            .iter()
            .map(|(p, i, a)| (p.round(), i.kind, (a * 100.0) as i32))
            .collect::<Vec<_>>(),
        blink_sig,
    );
    if built.map.as_ref() == Some(&map_sig) {
        for mut img in &mut nodes.quad {
            if img.rect != Some(src_rect) {
                img.rect = Some(src_rect);
            }
        }
    } else {
        built.map = Some(map_sig);
        ui.root = nodes.relayer(ui.commands, root, LocalLayer::Map);
        let src = URect::new(o.x as f32, o.y as f32, px.x as f32, px.y as f32);
        let e = ui.quad(target, pane, Some(src), Color::WHITE);
        ui.commands.entity(e).insert(LocalMapQuad);
        let zoom = win_state.zoom;
        for (p, i, angle) in &placed {
            let blink = win_state
                .blink
                .get(i.kind as usize)
                .copied()
                .unwrap_or(0.0)
                .max(0.0);
            let Some((img, ts)) = icon_image(&mut ui, i, true, blink > 0.0) else {
                continue;
            };
            // Custom: texture px × scale × 460 × s × 0.3125 / zoom; a blinking
            // icon grows by (ms left / 1000) × 20 × s (`DetailMapWidget::RenderLocalMapIcons`).
            let sz = if i.kind == 14 {
                ts.as_vec2() * i.scale * 460.0 * s * 0.3125 / zoom
            } else {
                Vec2::splat(isz + (blink * 0.001 * 20.0 * s).trunc())
            };
            icon_node(&mut ui, img, *p, sz, *angle, 1.0);
        }
    }
    let chrome_sig = (
        values.area_tag.clone(),
        win_state.legend_hover,
        win_state.button_hover,
        s.to_bits(),
    );
    if built.chrome.as_ref() != Some(&chrome_sig) {
        built.chrome = Some(chrome_sig);
        ui.root = nodes.relayer(ui.commands, root, LocalLayer::Chrome);
        let mut layout = LocalLayout::default();
        if let Some((img, sz)) = rec
            .str("backgroundImage")
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
        {
            ui.quad_stretch(
                img,
                URect::new(wr.x, wr.y, ui.sc(sz.x as f32), ui.sc(sz.y as f32)),
                Color::WHITE,
            );
        }
        let o = Vec2::new(wr.x, wr.y);
        if let Some(t) = ui.record(AREA_TITLE) {
            let tb = TextBox::from_record(&t);
            let name = ui.fonts.tag(&values.area_tag).to_owned();
            if !values.area_tag.is_empty() {
                ui.text(&tb, o, &name);
            }
        }
        let button = |ui: &mut Ui, path: &str, focus: bool, label: Option<&str>| -> Option<URect> {
            let b = ui.record(path)?;
            let up = b
                .str(if focus {
                    "bitmapNameInFocus"
                } else {
                    "bitmapNameUp"
                })
                .or(b.str("bitmapNameUp"))?
                .to_owned();
            let (img, sz) = ui.image(&up)?;
            // PORT: a labelled button grows to fit its label (the stock one is
            // fixed at its bitmap's width).
            let tex_w = ui.sc(sz.x as f32);
            let w = gd_ui_kit::widgets::button_width(ui, &b, tex_w, label);
            let r = URect::new(
                o.x + ui.sc(b.f32("bitmapPositionX").unwrap_or(0.0)),
                o.y + ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0)),
                w,
                ui.sc(sz.y as f32),
            );
            if w > tex_w {
                ui.quad_stretch(img, r, Color::WHITE);
            } else {
                ui.quad(img, r, None, Color::WHITE);
            }
            // The record's soundNameDown on press.
            ui.record_path_button_sound(r, path);
            Some(r)
        };
        for (n, k) in LEGEND.into_iter().enumerate() {
            // `bitmapNameInFocus` under the mouse.
            let path = format!("records/ui/mapaerial/iconbutton_{k}.dbr");
            if let Some(b) = ui.record(&path) {
                let key = if win_state.legend_hover == Some(n) {
                    "bitmapNameInFocus"
                } else {
                    "bitmapNameUp"
                };
                if let Some(img) = b.str(key).or(b.str("bitmapNameUp")).map(str::to_owned)
                    && let Some((_, r)) = ui.bitmap(
                        &img,
                        b.f32("bitmapPositionX").unwrap_or(0.0),
                        b.f32("bitmapPositionY").unwrap_or(0.0),
                        o,
                    )
                {
                    layout.legend.push((n, r));
                    ui.record_path_button_sound(r, &path);
                }
            }
            if let Some(t) = ui.record(&format!("records/ui/mapaerial/iconbutton_{k}text.dbr")) {
                let tb = TextBox::from_record(&t);
                let text = ui.fonts.tag(t.str("textTag").unwrap_or("")).to_owned();
                // PORT: the stock legend text overflows its fixed box on a long
                // localised label; here it truncates to the box's width.
                let text = match &tb.style {
                    Some(style) => ui.truncate_line(style, ui.sc(tb.rect.w), &text),
                    None => text,
                };
                ui.text(&tb, o, &text);
            }
        }
        layout.zoom = [
            button(
                &mut ui,
                "records/ui/mapaerial/map_buttonzoomin.dbr",
                false,
                None,
            ),
            button(
                &mut ui,
                "records/ui/mapaerial/map_buttonzoomout.dbr",
                false,
                None,
            ),
        ];
        // The map window's buttons (`UIWorldMap::Draw`): centre, switch ("Local Map"), close.
        if let Some(w) = data.db.get(crate::ui::riftmap::WINDOW) {
            layout.center = w
                .str("centerButton")
                .and_then(|p| button(&mut ui, p, win_state.button_hover == Some(1), None));
            if let Some(path) = w.str("switchMapButton") {
                let text = ui.fonts.tag("tagMapWindowName").to_owned();
                layout.switch = button(
                    &mut ui,
                    path,
                    win_state.button_hover == Some(2),
                    Some(&text),
                );
                if let (Some(r), Some(style)) = (
                    layout.switch,
                    data.db
                        .get(path)
                        .and_then(|b| b.str("style").map(str::to_owned)),
                ) {
                    gd_ui_kit::widgets::label(
                        &mut ui,
                        r,
                        &style,
                        gd_formats::ui::TextAlign::Center,
                        &text,
                    );
                }
            }
            layout.close = w
                .str("closeButton")
                .and_then(|p| button(&mut ui, p, win_state.button_hover == Some(0), None));
        }
        win_state.layout = layout;
    }
    win_state.layout.window = Some(wr);
    win_state.layout.pane = pane;
    win_state.layout.mpp = mpp;
    // The hovered icon's name, 3 px right of it, 5 px below its top.
    let label = hover.map(|h| (placed[h].1.name.clone(), placed[h].0.round()));
    if built.label.as_ref() != Some(&label) {
        ui.root = nodes.relayer(ui.commands, root, LocalLayer::Label);
        if let Some((name, p)) = &label {
            let lh = ui.line_height(NAME_STYLE);
            for (k, l) in name.lines().enumerate() {
                ui.text_line(
                    NAME_STYLE,
                    Vec2::new(
                        (p.x + isz * 0.5 + 3.0).round(),
                        (p.y - isz * 0.5 + 5.0 + k as f32 * lh).round(),
                    ),
                    l,
                    1.0,
                );
            }
        }
        built.label = Some(label);
    }
}
