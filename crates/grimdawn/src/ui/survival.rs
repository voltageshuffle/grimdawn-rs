//! The Crucible HUD (`GAME::UISurvivalHud`, Grim Dawn.exe load sub_1401DE270, render
//! sub_1401DDAA0, update sub_1401DDFD0) and its
//! objective window (`UIObjectiveWindow`, load sub_1401D02A0, render sub_1401CFF50).
//!
//! In `GameInfo::GetMode() == 1` the HUD (sub_1402139D0) shows both and hides the quest
//! tracker for the whole session. The pane (`hudSurvivalWindow`, `survivalpane_table.dbr`, 378x128 at
//! x 10 / y 40, right / top aligned) draws, in order: the background, the multiplier icon
//! (`survivalMultiplier1..10` by the 1..10 counter), the additive glows (multiplier / subtractor at
//! (232, -50), tribute at (46, 41), timer at (140, 22)), then the tribute, wave tier, score and timer
//! texts (plain integers, the timer "mm:ss" as `PlayStats::GetBonusFormattedTime`). Below 10 s the timer
//! text pulses red: alpha = triangle wave of `(time % 1750) / 1750 * 0.85 + 0.35` (folded above 0.775).
//! The glows start on GameEvents 63 (tribute gained: `tributeEarnedSound`), 64 (multiplier up: the
//! multiplier glow; reset: the subtractor glow and `timerFailedSound`) and 65 (timer add); all four use
//! the `multiplierpulse` record: alpha 1 -> 0 -> 1 -> 0 over three 350 ms legs. The objective window
//! (241 wide at y 320, right / top, black shade alpha 0.25, the quest tracker's border, `tagObjectives`
//! title, a bullet and the localised wrapped tag per objective) is drawn whenever visible, also empty.
//!
//! The timer warning sound re-triggers whenever the clip ends while the timer is under 10 s and stops when
//! the window leaves (the Crucible poll sub_1401DDFD0; the Shattered Realm's event 66 plays once per
//! entry — the clip (14.6 s) outlasts the window, so both sound the same, [`timer_warning`]); the five rollovers (`survivalpane_*rollover`) show through [`PaneRollovers`] (`rollover.rs`; the `^n^n` paragraph break of the timer text shows as one); the pane's update gate
//! (`IsGameWaiting`, engine+4848) is unknown; the timer text's red is a tint on the style colour (the
//! engine overrides the colour).
//!
//! The HUD shift (HUD render sub_14020A9D0, minimap mode switch `UIPlayerHud::ToggleCompass`
//! 0x140209DB0): the Crucible pane, the Shattered Realm pane, `hudInfoIconWindow` (the mutator icons) and
//! `hudChallengeAreaWindow` (`ui/challenge.rs`) are drawn at their window origin + HUD+704920 (x, y); their hover
//! tests reuse the drawn origin (sub_1401DDDC0 stores it at +3032), the objective window is not shifted.
//! Mode 2 (no minimap) sets (0, 0), mode 3 (the floating minimap) (−(160·s + 20), 0)
//! = the minimap's width and margin, mode 4 (−15 − HUD+22200, 0). See [`hud_shift`]; the mode is
//! `gd_game::compass::CompassMode` ([`compass_mode`]).
use super::{URect, Widget};
use super::{UiAssets, additive};
use crate::data::GameData;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{TextBox, WindowDef};

/// What the pane shows, from the story's survival state.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct SurvivalView {
    /// A Crucible session (the pane shows).
    pub active: bool,
    /// The objective window shows and the quest tracker hides: a Crucible session, or the player is in the
    /// Shattered Realm (`UIObjectiveWindow`, sub_14020D550: in the dungeon, outside mode 1).
    pub objective_window: bool,
    pub wave: u32,
    pub score: u32,
    pub tribute: u32,
    /// Bonus timer seconds (`ms / 1000`, the text's resolution) and whether it runs.
    pub timer_secs: i32,
    pub warning: bool,
    pub multiplier: u32,
    /// Objective localisation tags (`Game.AddObjective`).
    pub objectives: Vec<String>,
    /// The event counters (see `Survival::tribute_gains`).
    pub counters: [u32; 4],
}

/// Copies the story's survival state into [`SurvivalView`] (compared in place; it changes about once a second,
/// and the objective texts are copied only when they differ).
pub fn collect(
    mut fresh: gd_game::session::NewSession,
    rt: Option<NonSend<gd_game::story::StoryRuntime>>,
    mut view: ResMut<SurvivalView>,
) {
    if fresh.begun() {
        *view = SurvivalView::default();
    }
    let Some(rt) = rt else { return };
    let story = rt.story.borrow();
    let in_dungeon = story
        .player
        .region
        .is_some_and(|r| story.endless.in_dungeon(r, false));
    let survival = &story.game.survival;
    // The view without its objective texts, and whether it shows them.
    let (mut next, listed) = if story.game.survival_mode {
        let s = survival;
        (
            SurvivalView {
                active: true,
                objective_window: true,
                wave: s.wave_tier,
                score: s.score,
                tribute: story.player.tribute,
                timer_secs: s.timer_ms.max(0) / 1000,
                warning: s.timer_ms > 0 && s.timer_ms < 10_000,
                multiplier: s.multiplier.clamp(1, 10),
                objectives: Vec::new(),
                counters: [
                    s.tribute_gains,
                    s.multiplier_ups,
                    s.multiplier_resets,
                    s.timer_adds,
                ],
            },
            true,
        )
    } else if in_dungeon {
        // The Shattered Realm: the objective window only (the endless pane is `EndlessView`).
        (
            SurvivalView {
                objective_window: true,
                ..default()
            },
            true,
        )
    } else {
        (SurvivalView::default(), false)
    };
    let texts = || {
        survival
            .objectives
            .iter()
            .map(|(t, _)| t)
            .filter(|_| listed)
    };
    let same_texts = view.objectives.iter().eq(texts());
    // The rest compared with the texts set aside (no copy, no change mark).
    let cur = view.bypass_change_detection();
    let held = std::mem::take(&mut cur.objectives);
    let same_rest = *cur == next;
    cur.objectives = held;
    if same_texts && same_rest {
        return;
    }
    next.objectives = if same_texts {
        std::mem::take(&mut view.objectives)
    } else {
        texts().cloned().collect()
    };
    *view = next;
}

/// `GetBonusFormattedTime`: "mm:ss", "00:00" at zero.
pub fn timer_text(secs: i32) -> String {
    let s = secs.max(0);
    format!("{:02}:{:02}", s / 60 % 60, s % 60)
}

#[derive(Component)]
struct PaneRoot;
#[derive(Component)]
struct ObjectiveRoot;
#[derive(Component)]
struct TimerText;

/// Glow kinds, in the order of [`SurvivalView::counters`] events: multiplier (up), subtractor (reset),
/// tribute, timer.
const GLOWS: [(&str, [f32; 2]); 4] = [
    ("survivalMultiplierGlow", [232.0, -50.0]),
    ("survivalSubtractorGlow", [232.0, -50.0]),
    ("survivalTributeGlow", [46.0, 41.0]),
    ("survivalTimerGlow", [140.0, 22.0]),
];

#[derive(Component)]
struct Glow {
    kind: usize,
    image: Handle<Image>,
}

/// When each glow's pulse started (seconds of the app clock), the counters last seen, and each pane's
/// warning-sound voice ([`timer_warning`]).
#[derive(Resource, Default)]
pub struct GlowState {
    started: [Option<f32>; 4],
    seen: [u32; 4],
    warn_voice: Option<Entity>,
    endless_voice: Option<Entity>,
}

/// `ColorPulse` of `multiplierpulse` (A = alpha 0, B = alpha 1, 350 ms, one cycle): alpha 1 -> 0 -> 1 -> 0.
fn pulse_alpha(ms: f32) -> f32 {
    let leg = ms / 350.0;
    match leg {
        l if l < 0.0 => 0.0,
        l if l < 1.0 => 1.0 - l,
        l if l < 2.0 => l - 1.0,
        l if l < 3.0 => 3.0 - l,
        _ => 0.0,
    }
}

/// The timer warning's alpha (see the module doc).
fn warning_alpha(ms: u128) -> f32 {
    let mut x = (ms % 1750) as f32 / 1750.0 * 0.85 + 0.35;
    if x > 0.775 {
        x = 1.55 - x;
    }
    x
}

/// The endless pane's wider triangle (sub_1401BA3A0): `* 1.25`, folded at 0.975.
fn warning_alpha_endless(ms: u128) -> f32 {
    let mut x = (ms % 1750) as f32 / 1750.0 * 1.25 + 0.35;
    if x > 0.975 {
        x = 1.95 - x;
    }
    x
}

const MASTER: &str = super::hud::MASTER;

/// The timer warning sound (the Crucible `UISurvivalHud` update sub_1401DDFD0: while 0 < bonus <
/// 10 s, `if (pak && !SoundPak::IsPlaying(pak)) Play2D` — re-triggered whenever the clip ends; leaving the
/// window stops it, as the Shattered Realm's event-65 handler does with `SoundPak::Stop`, sub_1401BB780).
/// PORT: the pak's last voice entity being alive stands for `IsPlaying` (`audio::PeriodicSound`); the stop
/// is the voice's despawn (the engine's Crucible pane fades 500 ms instead).
fn timer_warning(
    commands: &mut Commands,
    data: &GameData,
    audio: &mut crate::audio::Sounds,
    alive: &Query<()>,
    voice: &mut Option<Entity>,
    warning: bool,
    pak: impl FnOnce() -> Option<String>,
) {
    let playing = voice.is_some_and(|e| alive.contains(e));
    if warning {
        if !playing && let Some(pak) = pak() {
            *voice = crate::audio::play_pak(commands, data, audio, &pak, None);
        }
    } else if let Some(e) = voice.take()
        && playing
    {
        commands.entity(e).try_despawn();
    }
}

/// Starts glows and sounds when an event counter rose, and the timer warning's sound.
#[allow(clippy::too_many_arguments)]
fn detect_events(
    view: Res<SurvivalView>,
    time: Res<Time>,
    data: Res<GameData>,
    mut state: ResMut<GlowState>,
    mut sounds: MessageWriter<gd_game::visuals::PlaySound>,
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    mut audio: ResMut<crate::audio::Sounds>,
    alive: Query<()>,
) {
    if fresh.begun() {
        *state = GlowState::default();
    }
    if !view.active {
        if let Some(e) = state.warn_voice.take() {
            commands.entity(e).try_despawn();
        }
        return;
    }
    // Looked up when a sound plays, not per frame.
    let table = std::cell::LazyCell::new(|| {
        data.db
            .get(MASTER)
            .and_then(|m| m.str("hudSurvivalWindow").map(str::to_owned))
            .and_then(|p| data.db.get(&p))
    });
    let sound = |k: &str| table.as_ref().and_then(|t| t.str(k)).map(str::to_owned);
    let now = time.elapsed_secs();
    let [gains, ups, resets, adds] = view.counters;
    let seen = state.seen;
    // Events 63 / 64 (true) / 64 (false) / 65 -> glow kinds tribute / multiplier / subtractor / timer.
    let events = [(gains, 2, 0), (ups, 0, 1), (resets, 1, 2), (adds, 3, 3)];
    for (count, glow, slot) in events {
        // The view and this state reset together with the session: any rise is an event.
        if count > seen[slot] {
            state.started[glow] = Some(now);
            let key = match slot {
                0 => Some("tributeEarnedSound"),
                2 => Some("timerFailedSound"),
                _ => None,
            };
            if let Some(pak) = key.and_then(sound) {
                sounds.write(gd_game::visuals::PlaySound { pak, at: None });
            }
        }
    }
    state.seen = view.counters;
    timer_warning(
        &mut commands,
        &data,
        &mut audio,
        &alive,
        &mut state.warn_voice,
        view.warning,
        || sound("timerTenSecondsSound"),
    );
}

/// Sets each glow's alpha from its pulse (additive material, rebuilt only for a changed 8-bit colour).
fn animate_glows(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<GlowState>,
    mut cache: ResMut<additive::AdditiveCache>,
    mut mats: ResMut<Assets<additive::AdditiveMaterial>>,
    mut glows: Query<(Entity, &Glow, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    for (e, g, mut vis) in &mut glows {
        let a = state.started[g.kind].map_or(0.0, |t| pulse_alpha((now - t) * 1000.0));
        if a <= 0.0 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        vis.set_if_neq(Visibility::Inherited);
        let h = cache.get(&mut mats, &g.image, Color::srgba(1.0, 1.0, 1.0, a));
        commands.entity(e).try_insert(MaterialNode(h));
    }
}

/// The timer text pulses red below 10 s (tint on the style colour).
fn animate_warning(
    view: Res<SurvivalView>,
    endless: Res<EndlessView>,
    clock: Res<Time<Real>>,
    mut texts: Query<(Option<&mut ImageNode>, Option<&mut TextColor>), With<TimerText>>,
) {
    let ms = clock.elapsed().as_millis();
    let tint = if view.warning {
        Color::srgba(1.0, 0.0, 0.0, warning_alpha(ms))
    } else if endless.warning {
        Color::srgba(1.0, 0.0, 0.0, warning_alpha_endless(ms))
    } else {
        Color::WHITE
    };
    for (img, col) in &mut texts {
        if let Some(mut i) = img {
            i.color = tint;
        }
        if let Some(mut c) = col {
            c.0 = tint;
        }
    }
}

/// Builds the pane (rebuilt when what it shows changes; the glows and the warning animate in place).
#[allow(clippy::too_many_arguments)]
fn build_pane(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    view: Res<SurvivalView>,
    roots: Query<Entity, With<PaneRoot>>,
    mut built: Local<Option<(UVec4, SurvivalView)>>,
    mut assets: UiAssets,
    mut zones: ResMut<PaneRollovers>,
    // Option: created once the player is in ([`compass_mode`]).
    compass: Option<Res<gd_game::compass::CompassMode>>,
) {
    if fresh.begun() {
        *built = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let mode = compass_mode(compass.as_deref());
    // The UI scale is a layout input too (an applied UI Scale option rebuilds).
    let sk = size
        .extend(mode as u32)
        .extend(assets.scale(size).to_bits());
    // Nothing changed: skip the key's clone of the view.
    if !view.is_changed() && built.as_ref().is_some_and(|(s, _)| *s == sk) {
        return;
    }
    let mut key = view.clone();
    // Not layout inputs: the counters and the objectives (another root) only animate / rebuild elsewhere.
    key.counters = [0; 4];
    key.objectives.clear();
    if size.x == 0 || built.as_ref().is_some_and(|(s, v)| *s == sk && *v == key) {
        return;
    }
    *built = Some((sk, key));
    zones.crucible.clear();
    for e in &roots {
        commands.entity(e).try_despawn();
    }
    if !view.active {
        return;
    }
    let root = crate::ui::spawn_hud_overlay(&mut commands, PaneRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let Some(table_path) = ui
        .record(MASTER)
        .and_then(|m| m.str("hudSurvivalWindow").map(str::to_owned))
    else {
        return;
    };
    let Some(table) = ui.record(&table_path) else {
        return;
    };
    let field = |k: &str| table.str(k).unwrap_or("").to_owned();
    let pane = WindowDef::from_record(&table).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let origin = Vec2::new(pane.x, pane.y) + hud_shift(ui.s, mode);
    let bitmap = |ui: &mut super::Ui, rec: &str| {
        if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(rec) {
            ui.bitmap(&bitmap, x, y, origin);
        }
    };
    bitmap(&mut ui, &field("survivalBackground"));
    let multiplier = field(&format!(
        "survivalMultiplier{}",
        view.multiplier.clamp(1, 10)
    ));
    bitmap(&mut ui, &multiplier);
    // The rollover areas, in the engine's test order: multiplier icon, score, timer, tribute, wave tier.
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&multiplier)
        && let Some((_, px)) = ui.image(&bitmap)
    {
        let rect = URect::new(
            origin.x + ui.sc(x),
            origin.y + ui.sc(y),
            ui.sc(px.x as f32),
            ui.sc(px.y as f32),
        );
        zones.crucible.push(Zone {
            field: "survivalMultiplierRollover",
            rect,
            record: field("survivalMultiplierRollover"),
            lines: None,
        });
    }
    for (name, rollover) in [
        ("survivalScore", "survivalScoreRollover"),
        ("survivalTimer", "survivalTimerRollover"),
        ("survivalTributes", "survivalTributeRollover"),
        ("survivalWaveTier", "survivalWaveTierRollover"),
    ] {
        if let Some(r) = ui.record(&field(name)) {
            let b = TextBox::from_record(&r).rect;
            zones.crucible.push(Zone {
                field: rollover,
                rect: URect::new(
                    origin.x + ui.sc(b.x),
                    origin.y + ui.sc(b.y),
                    ui.sc(b.w),
                    ui.sc(b.h),
                ),
                record: field(rollover),
                lines: None,
            });
        }
    }
    // The glows: always built, shown by `animate_glows`.
    for (kind, (name, at)) in GLOWS.iter().enumerate() {
        let Some(Widget::Bitmap { bitmap, .. }) = ui.widget(&field(name)) else {
            continue;
        };
        let Some((image, px)) = ui.image(&bitmap) else {
            continue;
        };
        let r = URect::new(
            origin.x + ui.sc(at[0]),
            origin.y + ui.sc(at[1]),
            ui.sc(px.x as f32),
            ui.sc(px.y as f32),
        );
        let e = ui.commands.spawn((ui.node(r), Visibility::Hidden)).id();
        ui.commands.entity(e).insert(Glow { kind, image });
        ui.commands.entity(ui.root).add_child(e);
    }
    let text = |ui: &mut super::Ui, rec: &str, s: String| -> Option<Entity> {
        let r = ui.record(rec)?;
        let tb = TextBox::from_record(&r);
        ui.text(&tb, origin, &s)
    };
    text(
        &mut ui,
        &field("survivalTributes"),
        view.tribute.to_string(),
    );
    text(&mut ui, &field("survivalWaveTier"), view.wave.to_string());
    text(&mut ui, &field("survivalScore"), view.score.to_string());
    if let Some(e) = text(
        &mut ui,
        &field("survivalTimer"),
        timer_text(view.timer_secs),
    ) {
        ui.commands.entity(e).try_insert(TimerText);
    }
}

/// Builds the objective window (rebuilt when the list or the screen changes).
#[allow(clippy::too_many_arguments)]
fn build_objectives(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    view: Res<SurvivalView>,
    roots: Query<Entity, With<ObjectiveRoot>>,
    mut built: Local<Option<(UVec2, u32, bool, Vec<String>)>>,
    mut assets: UiAssets,
) {
    if fresh.begun() {
        *built = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let sb = assets.scale(size).to_bits();
    if size.x == 0
        || built.as_ref().is_some_and(|(s, b, w, o)| {
            *s == size && *b == sb && *w == view.objective_window && *o == view.objectives
        })
    {
        return;
    }
    *built = Some((size, sb, view.objective_window, view.objectives.clone()));
    for e in &roots {
        commands.entity(e).try_despawn();
    }
    if !view.objective_window {
        return;
    }
    let root = crate::ui::spawn_hud_overlay(&mut commands, ObjectiveRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let Some(rec) = ui
        .record(MASTER)
        .and_then(|m| m.str("hudObjectiveWindow").map(str::to_owned))
        .and_then(|p| ui.record(&p))
    else {
        return;
    };
    let field = |k: &str| rec.str(k).unwrap_or("").to_owned();
    let wr = WindowDef::from_record(&rec).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let (title_style, obj_style) = (field("titleFontStyle"), field("objectiveFontStyle"));
    let (title_lh, obj_lh) = (ui.line_height(&title_style), ui.line_height(&obj_style));
    let bullet = match ui.widget(&field("objectiveBullet")) {
        Some(Widget::Bitmap { bitmap, .. }) => ui.image(&bitmap),
        _ => None,
    };
    let bullet_w = bullet.as_ref().map_or(0.0, |(_, s)| ui.sc(s.x as f32));
    // Lay out (the height goes into the shade): the title line, 3 px, then the wrapped objectives.
    let mut lines: Vec<(f32, f32, String, bool)> = Vec::new();
    let mut y = title_lh + 3.0;
    let mut height = title_lh;
    for tag in &view.objectives {
        let text = ui.fonts.tag(tag).to_owned();
        for (i, l) in ui
            .wrap_lines(&obj_style, wr.w - bullet_w, &text)
            .into_iter()
            .enumerate()
        {
            lines.push((y, bullet_w, l, i == 0));
            y += obj_lh;
            height += obj_lh;
        }
    }
    let r = URect::new(wr.x, wr.y, wr.w, height.round());
    ui.solid(r, Color::srgba(0.0, 0.0, 0.0, 0.25));
    ui.border(&rec, "", r, 1.0);
    let title = ui.fonts.tag(&field("titleTag")).to_owned();
    ui.text_line(&title_style, Vec2::new(r.x, r.y), &title, 1.0);
    for (ly, x, text, first) in lines {
        if first && let Some((img, s)) = bullet.clone() {
            let (w, h) = (ui.sc(s.x as f32), ui.sc(s.y as f32));
            ui.quad(img, URect::new(r.x, r.y + ly, w, h), None, Color::WHITE);
        }
        ui.text_line(&obj_style, Vec2::new(r.x + x, r.y + ly), &text, 1.0);
    }
}

/// What the Shattered Realm's pane shows (`UIEndlessHud`, exe load `sub_1401BAC90`, update `sub_1401BA3A0`, render
/// `sub_1401B9F80`). Loaded from `hudEndlessDungeonWindow`
/// (`endlesshud_table.dbr`, 384x190 at x 10 / y 3, right / top), visible while the player's region belongs to a
/// floor of the dungeon. Drawn in order: the progress bar (the floor's progress / total, right-anchored animated
/// sheet with its empty bitmap and cap, `hud::Stat::EndlessProgress`) and the always-full orb (`Stat::Full`), the
/// background, the timer glow (a 350 ms pulse on every timer add), the texts: "Shard" and the dungeons generated, "Chunk"
/// and the floor number + 1 (0-based index, the boss floor included), the bonus timer "mm:ss" (the Crucible's
/// timer; below 10 s it pulses red: alpha = triangle of `(t % 1750) / 1750 * 1.25 + 0.35` folded at 0.975).
/// The objective window shows the floor's tag (`endless::sync_objectives`). Hover sub_1401BA210: the
/// rollovers test each text's own box (the bar first, its result dropped), the Shard name / number ->
/// `endlessDungeonNameRollover`, the Chunk name / number -> `endlessFloorNameRollover`, the timer ->
/// `endlessTimerRollover` (`RolloverLine2`, through [`PaneRollovers`]; PORT: the timer text's `^n^n` paragraph
/// breaks show as one, as in the Crucible's). The events: `detect_endless_events`. PORT: the timer's style record
/// (`style_survivalpane_s`) exists only in the Crucible's mod, so `style_endlessdungeon_s` stands in outside it.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct EndlessView {
    pub active: bool,
    /// Floor index + 1 and the dungeons generated.
    pub floor: u32,
    pub shard: u32,
    pub timer_secs: i32,
    pub warning: bool,
    pub timer_adds: u32,
    /// Event 64 with a timer that was out (`Survival::timer_failures`) and event 65 with a loss (`timer_cuts`).
    pub timer_failures: u32,
    pub timer_cuts: u32,
    /// The dungeon's active floor index and whether its boss is dead (event 67's floor / boss complete sounds).
    pub active_floor: u32,
    pub boss_dead: bool,
}

#[derive(Component)]
struct EndlessRoot;

/// The endless pane's state from the story.
pub fn collect_endless(
    mut fresh: gd_game::session::NewSession,
    rt: Option<NonSend<gd_game::story::StoryRuntime>>,
    mut view: ResMut<EndlessView>,
) {
    if fresh.begun() {
        *view = EndlessView::default();
    }
    let Some(rt) = rt else { return };
    let s = rt.story.borrow();
    let region = s.player.region.filter(|r| s.endless.in_dungeon(*r, false));
    let next = match region {
        Some(r) => EndlessView {
            active: true,
            floor: s.endless.floor_of(r).map_or(0, |i| i as u32 + 1),
            shard: s.endless.generated,
            timer_secs: s.game.survival.timer_ms.max(0) / 1000,
            warning: s.game.survival.timer_ms > 0 && s.game.survival.timer_ms < 10_000,
            timer_adds: s.game.survival.timer_adds,
            timer_failures: s.game.survival.timer_failures,
            timer_cuts: s.game.survival.timer_cuts,
            active_floor: s.endless.dungeon.as_ref().map_or(0, |d| d.active as u32),
            boss_dead: s.endless.boss_dead,
        },
        None => EndlessView::default(),
    };
    if *view != next {
        *view = next;
    }
}

/// The endless HUD's events (`UIEndlessHud`; all only while the pane shows): every
/// timer change by `IncrementBonusTimer` (event 65, handler exe sub_1401BB780) pulses the timer glow, a positive
/// one also plays `bonusTimeSound` (a death's penalty only pulses); the glow also pulses once when the bonus
/// first drops to 60 s or less (the latch clears above it); the timer under 10 s plays `timerTenSecondsSound`
/// ([`timer_warning`]: re-triggered while it ends, as the Crucible pane — the engine's event 66 plays once per
/// entry, the clip outlasts the window); event 64 with its byte false (handler sub_1401BBA10; the timer ran out, or
/// the boss's timer reset found it out) plays `timerFailedSound`; a floor completing (event 67, sub_1401BB850)
/// plays `floorCompleteSound`, the boss's death `bossCompleteSound` (its tutorial page: `gd_game::endless`).
#[allow(clippy::too_many_arguments)]
fn detect_endless_events(
    view: Res<EndlessView>,
    time: Res<Time>,
    data: Res<GameData>,
    mut state: ResMut<GlowState>,
    mut sounds: MessageWriter<gd_game::visuals::PlaySound>,
    mut fresh: gd_game::session::NewSession,
    // The timer adds, failures and cuts last seen while the pane showed (the view zeroes them while hidden).
    mut seen: Local<Option<[u32; 3]>>,
    // The active floor and the boss flag last seen, and the 60 s latch.
    mut last: Local<Option<(u32, bool)>>,
    mut low: Local<bool>,
    mut commands: Commands,
    mut audio: ResMut<crate::audio::Sounds>,
    alive: Query<()>,
) {
    let counters = [view.timer_adds, view.timer_failures, view.timer_cuts];
    if fresh.begun() {
        *seen = None;
        *last = None;
        *low = false;
        state.endless_voice = None;
    }
    if !view.active {
        *seen = None;
        if let Some(e) = state.endless_voice.take() {
            commands.entity(e).try_despawn();
        }
        return;
    }
    // Looked up when a sound plays, not per frame.
    let table = std::cell::LazyCell::new(|| {
        data.db
            .get(MASTER)
            .and_then(|m| m.str("hudEndlessDungeonWindow").map(str::to_owned))
            .and_then(|p| data.db.get(&p))
    });
    let mut play = |key: &str| {
        if let Some(pak) = table.as_ref().and_then(|t| t.str(key)) {
            sounds.write(gd_game::visuals::PlaySound {
                pak: pak.to_owned(),
                at: None,
            });
        }
    };
    let now = time.elapsed_secs();
    let [adds, failures, cuts] = seen.unwrap_or(counters);
    if view.timer_adds > adds {
        state.started[3] = Some(now);
        play("bonusTimeSound");
    }
    if view.timer_cuts > cuts {
        state.started[3] = Some(now);
    }
    if view.timer_failures > failures {
        play("timerFailedSound");
    }
    *seen = Some(counters);
    let running = view.timer_secs > 0 || view.warning;
    let under_minute = running && view.timer_secs <= 60;
    if under_minute && !*low {
        state.started[3] = Some(now);
    }
    *low = under_minute || (*low && view.timer_secs <= 60);
    timer_warning(
        &mut commands,
        &data,
        &mut audio,
        &alive,
        &mut state.endless_voice,
        view.warning,
        || {
            table
                .as_ref()
                .and_then(|t| t.str("timerTenSecondsSound"))
                .map(str::to_owned)
        },
    );
    if let Some((floor, boss)) = *last {
        if view.active_floor > floor {
            play("floorCompleteSound");
        }
        if view.boss_dead && !boss {
            play("bossCompleteSound");
        }
    }
    *last = Some((view.active_floor, view.boss_dead));
}

/// Builds the endless pane (rebuilt when what it shows changes).
#[allow(clippy::too_many_arguments)]
fn build_endless_pane(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    view: Res<EndlessView>,
    roots: Query<Entity, With<EndlessRoot>>,
    mut built: Local<Option<(UVec4, EndlessView)>>,
    mut assets: UiAssets,
    mut zones: ResMut<PaneRollovers>,
    // Option: created once the player is in ([`compass_mode`]).
    compass: Option<Res<gd_game::compass::CompassMode>>,
) {
    if fresh.begun() {
        *built = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let mode = compass_mode(compass.as_deref());
    let sk = size
        .extend(mode as u32)
        .extend(assets.scale(size).to_bits());
    if size.x == 0 || built.as_ref().is_some_and(|(s, v)| *s == sk && *v == *view) {
        return;
    }
    *built = Some((sk, view.clone()));
    zones.endless.clear();
    for e in &roots {
        commands.entity(e).try_despawn();
    }
    if !view.active {
        return;
    }
    let root = crate::ui::spawn_hud_overlay(&mut commands, EndlessRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let Some(table) = ui
        .record(MASTER)
        .and_then(|m| m.str("hudEndlessDungeonWindow").map(str::to_owned))
        .and_then(|p| ui.record(&p))
    else {
        return;
    };
    let field = |k: &str| table.str(k).unwrap_or("").to_owned();
    let pane = WindowDef::from_record(&table).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let origin = Vec2::new(pane.x, pane.y) + hud_shift(ui.s, mode);
    super::hud::bar_field(
        &mut ui,
        &field("endlessProgressBar"),
        origin,
        super::hud::Stat::EndlessProgress,
    );
    super::hud::bar_field(
        &mut ui,
        &field("endlessOrbBitmap"),
        origin,
        super::hud::Stat::Full,
    );
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&field("endlessBackground")) {
        ui.bitmap(&bitmap, x, y, origin);
    }
    // The timer glow (shown by `animate_glows`, kind 3).
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&field("endlessLowTimeGlow"))
        && let Some((image, px)) = ui.image(&bitmap)
    {
        let r = URect::new(
            origin.x + ui.sc(x),
            origin.y + ui.sc(y),
            ui.sc(px.x as f32),
            ui.sc(px.y as f32),
        );
        let e = ui.commands.spawn((ui.node(r), Visibility::Hidden)).id();
        ui.commands.entity(e).insert(Glow { kind: 3, image });
        ui.commands.entity(ui.root).add_child(e);
    }
    // The rollover areas: the shard texts, the chunk texts, the timer (the bar and orb have none).
    for (names, rollover) in [
        (
            ["endlessDungeonName", "endlessDungeonCounter"],
            "endlessDungeonNameRollover",
        ),
        (
            ["endlessFloorName", "endlessFloorCounter"],
            "endlessFloorNameRollover",
        ),
        (["endlessTimer", "endlessTimer"], "endlessTimerRollover"),
    ] {
        for name in &names[..if names[0] == names[1] { 1 } else { 2 }] {
            if let Some(r) = ui.record(&field(name)) {
                let b = TextBox::from_record(&r).rect;
                zones.endless.push(Zone {
                    field: rollover,
                    rect: URect::new(
                        origin.x + ui.sc(b.x),
                        origin.y + ui.sc(b.y),
                        ui.sc(b.w),
                        ui.sc(b.h),
                    ),
                    record: field(rollover),
                    lines: None,
                });
            }
        }
    }
    let text = |ui: &mut super::Ui, rec: &str, s: Option<String>| -> Option<Entity> {
        let r = ui.record(rec)?;
        let mut tb = TextBox::from_record(&r);
        // The timer's style record is the Crucible mod's; fall back to the number style.
        if tb
            .style
            .as_deref()
            .is_none_or(|p| ui.data.db.get(p).is_none())
        {
            tb.style = Some("records/ui/styles/text/style_endlessdungeon_s.dbr".to_owned());
        }
        let s = s.or_else(|| tb.tag.as_ref().map(|t| ui.fonts.tag(t).to_owned()))?;
        ui.text(&tb, origin, &s)
    };
    text(&mut ui, &field("endlessDungeonName"), None);
    text(&mut ui, &field("endlessFloorName"), None);
    text(
        &mut ui,
        &field("endlessDungeonCounter"),
        Some(view.shard.to_string()),
    );
    text(
        &mut ui,
        &field("endlessFloorCounter"),
        Some(view.floor.to_string()),
    );
    if let Some(e) = text(
        &mut ui,
        &field("endlessTimer"),
        Some(timer_text(view.timer_secs)),
    ) {
        ui.commands.entity(e).try_insert(TimerText);
    }
}

/// A rollover area of a pane (hover tests `sub_1401DDDC0` / `sub_1401BA210`): the first area under the cursor
/// shows its `RolloverLine2` record (`field` names the table field for the `GD_HUD_HOVER` hook).
pub struct Zone {
    pub field: &'static str,
    pub rect: URect,
    pub record: String,
    /// A rollover with its own lines (style, text, wrapped) instead of a `RolloverLine2` record (mutator icons).
    pub lines: Option<Vec<(String, String, bool)>>,
}

/// The panes' rollover areas in hit-test order, set when a pane is built (`rollover.rs::hud_rollover` reads them).
#[derive(Resource, Default)]
pub struct PaneRollovers {
    pub crucible: Vec<Zone>,
    pub endless: Vec<Zone>,
    pub mutators: Vec<Zone>,
    /// The challenge-area window's name (`challenge.rs`).
    pub challenge: Vec<Zone>,
}

/// One mutator icon shown (`Game.AddMutator` list).
#[derive(Clone, PartialEq)]
struct MutatorIcon {
    bitmap: String,
    name: String,
    description: String,
    /// The pak's attribute lines (localised, `^X` codes kept).
    attributes: Vec<String>,
}

/// The mutator icons shown.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct MutatorView {
    icons: Vec<MutatorIcon>,
}

#[derive(Component)]
struct MutatorRoot;

/// The mutators the info icon window lists (`UIInfoIconWindow` @0x1401CADE0):
/// always in the Crucible, in the Shattered Realm while the player is in the dungeon (the session's list), else
/// in a challenge area that shows the HUD (the area's, `challenge.rs`; type 2 with its id). An icon's attribute lines
/// (rollover entry `sub_1401CA110`): the mutator's pak of the game difficulty (`normalMutator` /
/// `eliteMutator` / `ultimateMutator`) at level 1 when it `affectsPlayers`, else at the main player's character
/// level, printed as an `AttributePak`'s text (class 19, `items::tooltip::attribute_line_rich` in store order).
fn collect_mutators(
    mut fresh: gd_game::session::NewSession,
    rt: Option<NonSend<gd_game::story::StoryRuntime>>,
    data: Res<GameData>,
    hud: Res<super::HudValues>,
    mode: Res<gd_game::session::GameMode>,
    mut view: ResMut<MutatorView>,
    // The inputs the view was last computed from (mutator list, difficulty, character level): the db lookups
    // and attribute resolution run only when one changes.
    mut last: Local<Option<(Vec<String>, usize, u32)>>,
) {
    if fresh.begun() {
        *view = MutatorView::default();
        *last = None;
    }
    let Some(rt) = rt else { return };
    let s = rt.story.borrow();
    let shown = s.game.survival_mode
        || s.player
            .region
            .is_some_and(|r| s.endless.in_dungeon(r, false));
    let list = if shown {
        Some(&s.game.survival.mutators)
    } else {
        hud.challenge_area.as_ref().map(|c| &c.mutators)
    };
    let empty = Vec::new();
    let inputs = (list.unwrap_or(&empty), mode.0.difficulty, hud.level);
    if last
        .as_ref()
        .is_some_and(|(l, d, lv)| l == inputs.0 && *d == inputs.1 && *lv == inputs.2)
    {
        return;
    }
    *last = Some((inputs.0.clone(), inputs.1, inputs.2));
    let icons = if let Some(list) = list {
        let tags = data.tags();
        let key = ["normalMutator", "eliteMutator"]
            .get(mode.0.difficulty)
            .copied()
            .unwrap_or("ultimateMutator");
        list.iter()
            .filter_map(|m| {
                let r = data.db.get(m)?;
                let level = if r.f32("affectsPlayers").is_some_and(|v| v != 0.0) {
                    1
                } else {
                    hud.level.max(1)
                };
                let attributes = r
                    .str(key)
                    .and_then(|p| data.db.get(p))
                    .map(|pak| {
                        use gd_sim::items::{resolve, tooltip};
                        let mut attrs = resolve::record_attrs_at(
                            pak,
                            resolve::Source::Base,
                            level as usize - 1,
                        );
                        attrs.sort_by_key(tooltip::store_rank);
                        attrs
                            .iter()
                            .filter_map(|a| tooltip::attribute_line_rich(&tags, a))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(MutatorIcon {
                    bitmap: r
                        .str("bitmapName")
                        .filter(|b| !b.is_empty())
                        .unwrap_or("ui/infoiconpane/generic_mutatoricon.tex")
                        .to_owned(),
                    name: r.str("mutatorName").unwrap_or("").to_owned(),
                    description: r.str("description").unwrap_or("").to_owned(),
                    attributes,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    if view.icons != icons {
        view.icons = icons;
    }
}

/// Builds the mutator icons (render `sub_1401CAAA0`: the window `hudInfoIconWindow`, right / top; icons laid
/// right to left from its right edge, window x + width (+72, the extent `LoadBase` sub_1402617D0 sets), with
/// `iconSpacing`, no background or text). Rollover (`sub_1401CA110`): the name in text class 7, the
/// description (when set) in class 17, the attribute lines in class 19 (`GameEngine::GetGameTextStyleName`: the
/// `gameengine.dbr` keys of `gd_sim::items::tooltip::CLASS_KEYS`).
#[allow(clippy::too_many_arguments)]
fn build_mutators(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    view: Res<MutatorView>,
    roots: Query<Entity, With<MutatorRoot>>,
    mut built: Local<Option<(UVec4, MutatorView)>>,
    mut assets: UiAssets,
    mut zones: ResMut<PaneRollovers>,
    // Option: created once the player is in ([`compass_mode`]).
    compass: Option<Res<gd_game::compass::CompassMode>>,
) {
    if fresh.begun() {
        *built = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let mode = compass_mode(compass.as_deref());
    let sk = size
        .extend(mode as u32)
        .extend(assets.scale(size).to_bits());
    if size.x == 0 || built.as_ref().is_some_and(|(s, v)| *s == sk && *v == *view) {
        return;
    }
    *built = Some((sk, view.clone()));
    zones.mutators.clear();
    for e in &roots {
        commands.entity(e).try_despawn();
    }
    if view.icons.is_empty() {
        return;
    }
    let root = crate::ui::spawn_hud_overlay(&mut commands, MutatorRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let Some(table) = ui
        .record(MASTER)
        .and_then(|m| m.str("hudInfoIconWindow").map(str::to_owned))
        .and_then(|p| ui.record(&p))
    else {
        return;
    };
    let pane = WindowDef::from_record(&table).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let spacing = ui.sc(table.f32("iconSpacing").unwrap_or(6.0));
    let shift = hud_shift(ui.s, mode);
    let mut x = pane.x + pane.w + shift.x;
    let engine = ui.record("records/game/gameengine.dbr");
    let style = |class: usize| {
        engine
            .as_ref()
            .and_then(|e| e.str(gd_sim::items::tooltip::CLASS_KEYS[class]))
            .unwrap_or("records/ui/styles/text/style_textwhite_sizen_bold.dbr")
            .to_owned()
    };
    for icon in &view.icons {
        let Some((image, px)) = ui.image(&icon.bitmap) else {
            continue;
        };
        let (w, h) = (ui.sc(px.x as f32), ui.sc(px.y as f32));
        x -= w;
        let rect = URect::new(x, pane.y + shift.y, w, h);
        ui.quad(image, rect, None, Color::WHITE);
        let mut lines = vec![(style(7), ui.fonts.tag(&icon.name).to_owned(), false)];
        if !icon.description.is_empty() {
            lines.push((style(17), ui.fonts.tag(&icon.description).to_owned(), true));
        }
        lines.extend(icon.attributes.iter().map(|t| (style(19), t.clone(), true)));
        zones.mutators.push(Zone {
            field: "mutatorIcon",
            rect,
            record: super::lootfilter::RECORD.to_owned(),
            lines: Some(lines),
        });
        x -= spacing;
    }
}

/// The compass mode the panes shift for (`gd_game::compass`; a new character's before the player is in).
pub(crate) fn compass_mode(c: Option<&gd_game::compass::CompassMode>) -> u8 {
    c.map_or(gd_game::compass::NEW_CHARACTER_MODE, |c| c.0)
}

/// The HUD shift for compass `mode` (`UIPlayerHud::ToggleCompass` 0x140209DB0, leftward; module doc): mode
/// 2 none, mode 4 `15 + (int)(246·s)` (HUD+22200 is the minimap widget's width, set by sub_140174E10), else (mode 3;
/// modes 0 / 1 leave the shift as it was, the port's 3) the minimap's 160 × s side plus the 20 px margin, unrounded.
pub(crate) fn hud_shift(s: f32, mode: u8) -> Vec2 {
    match mode {
        2 => Vec2::ZERO,
        4 => Vec2::new(-15.0 - (246.0 * s).trunc(), 0.0),
        _ => Vec2::new(-(160.0 * s + 20.0), 0.0),
    }
}

/// `Game.PlayFullscreenGlow` (`GameEngine::ShowGlowEffect` @0x1802fa6b0 -> `InGameUI` glow): a trigger sets
/// the target to 1 when it is 0; the level follows it at 1 per second and the
/// target falls back to 0 on reaching 1, so the glow is 0 -> 1 -> 0 over about 2 s.
#[derive(Resource, Default)]
pub struct GlowOverlay {
    level: f32,
    target: f32,
}

/// The `InGameUI` overlay ramp (`sub_14002BC00`): the level follows its target at 1 per second; on
/// reaching 1 the target falls back to 0, so a trigger makes a 2 s triangle 0 -> 1 -> 0.
fn ramp(level: &mut f32, target: &mut f32, dt: f32) {
    if *level < *target {
        *level = (*level + dt).min(*target);
        if *level >= 1.0 {
            *target = 0.0;
        }
    } else if *level > *target {
        *level = (*level - dt).max(*target);
    }
}

#[derive(Component)]
struct GlowOverlayRoot;

#[derive(Component)]
struct GlowOverlayNode;

/// The fullscreen glow: the texture `ui/hud/wavecompleted_glowoverlay01.tex` (hardcoded in the engine) over the
/// whole screen in white with alpha `clamp(level + 0.1 sin(t * 0.003141593 per ms), 0, 1)` (a shimmer of period
/// 2 s), only with the `screenEffects` option (bool 66). PORT: plain alpha blending (the canvas default state's
/// blend is not read).
#[allow(clippy::too_many_arguments)]
fn fullscreen_glow(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    mut effects: MessageReader<gd_game::story::StoryEffect>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    clock: Res<Time<Real>>,
    mut glow: ResMut<GlowOverlay>,
    roots: Query<Entity, With<GlowOverlayRoot>>,
    mut nodes: Query<(&mut ImageNode, &mut Visibility), With<GlowOverlayNode>>,
    mut built: Local<Option<UVec2>>,
    mut assets: UiAssets,
    options: Res<gd_game::options::GameOptions>,
) {
    if fresh.begun() {
        *glow = GlowOverlay::default();
        *built = None;
    }
    for e in effects.read() {
        if matches!(e.0, gd_sim::story::Effect::FullscreenGlow) && glow.target == 0.0 {
            debug!("ui: fullscreen glow");
            glow.target = 1.0;
        }
    }
    let glow = &mut *glow;
    ramp(&mut glow.level, &mut glow.target, clock.delta_secs());
    // Drawn only with the `screenEffects` option (`Options::GetBool(66)`, `InGameUI` render).
    if (glow.level <= 0.0 && glow.target == 0.0) || !options.flag("screenEffects") {
        for (_, mut v) in &mut nodes {
            v.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if *built != Some(size) || nodes.is_empty() {
        *built = Some(size);
        for e in &roots {
            commands.entity(e).try_despawn();
        }
        let root = crate::ui::spawn_root(&mut commands, GlowOverlayRoot, 3);
        let mut ui = assets.ui(
            &mut commands,
            &data,
            root,
            assets.scale(size),
            1.0 / win.scale_factor(),
            Vec2::new(size.x as f32, size.y as f32),
        );
        debug!("ui: fullscreen glow overlay built for {size}");
        if let Some((image, _)) = ui.image("ui/hud/wavecompleted_glowoverlay01.tex") {
            let e = ui.quad_stretch(
                image,
                URect::new(0.0, 0.0, size.x as f32, size.y as f32),
                Color::srgba(1.0, 1.0, 1.0, 0.0),
            );
            ui.commands.entity(e).try_insert(GlowOverlayNode);
        }
        return;
    }
    let ms = clock.elapsed().as_millis() as f32;
    let alpha = ((ms * 0.003_141_593).sin() * 0.1 + glow.level).clamp(0.0, 1.0);
    for (mut img, mut v) in &mut nodes {
        img.color = Color::srgba(1.0, 1.0, 1.0, alpha);
        v.set_if_neq(Visibility::Inherited);
    }
}

/// The "sundered" overlay's ramp state (`InGameUI` +2464 level, +2468 target).
#[derive(Resource, Default)]
pub struct SunderedOverlay {
    level: f32,
    target: f32,
}

#[derive(Component)]
struct SunderedRoot;

#[derive(Component)]
struct SunderedNode;

/// The "sundered" overlay (`InGameUI` frame `sub_14002BC00` / render `sub_140027350`): while the player's
/// damage multiplier accumulator
/// (`Character::GetDamageMultiplier` = `DurationDamageManager::GetFixedDamage(61)`: the player's type-61 duration entry,
/// applied by enemy skills' `offensiveSlowDamageMult*`) is nonzero and the target is 0 the target becomes 1: the level then repeats the 2 s
/// triangle ([`ramp`]) while the condition holds. `ui/hud/hud_statusoverlay_sundered01.tex` over the whole
/// screen above the HUD with alpha `clamp(sin(t * 0.003141593 per ms) * 0.3 + level, 0, 1)`, only with the
/// `screenEffects` option. PORT: plain alpha blending. `negativeDamageMult` (the Amplification potions) does not feed it; enemy skills'
/// `offensiveSlowDamageMult*` give the player the entry (`gd_game::afflictions`).
#[allow(clippy::too_many_arguments)]
fn sundered_overlay(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    clock: Res<Time<Real>>,
    values: Res<super::HudVitals>,
    mut state: ResMut<SunderedOverlay>,
    roots: Query<Entity, With<SunderedRoot>>,
    mut nodes: Query<(&mut ImageNode, &mut Visibility), With<SunderedNode>>,
    mut built: Local<Option<UVec2>>,
    mut assets: UiAssets,
    options: Res<gd_game::options::GameOptions>,
) {
    if fresh.begun() {
        *state = SunderedOverlay::default();
        *built = None;
    }
    let state = &mut *state;
    if values.damage_taken_debuff != 0.0 && state.target == 0.0 {
        state.target = 1.0;
    }
    ramp(&mut state.level, &mut state.target, clock.delta_secs());
    if (state.level <= 0.0 && state.target == 0.0) || !options.flag("screenEffects") {
        for (_, mut v) in &mut nodes {
            v.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if *built != Some(size) || nodes.is_empty() {
        *built = Some(size);
        for e in &roots {
            commands.entity(e).try_despawn();
        }
        let root = crate::ui::spawn_root(&mut commands, SunderedRoot, 3);
        let mut ui = assets.ui(
            &mut commands,
            &data,
            root,
            assets.scale(size),
            1.0 / win.scale_factor(),
            Vec2::new(size.x as f32, size.y as f32),
        );
        if let Some((image, _)) = ui.image("ui/hud/hud_statusoverlay_sundered01.tex") {
            let e = ui.quad_stretch(
                image,
                URect::new(0.0, 0.0, size.x as f32, size.y as f32),
                Color::srgba(1.0, 1.0, 1.0, 0.0),
            );
            ui.commands.entity(e).try_insert(SunderedNode);
        }
        return;
    }
    let ms = clock.elapsed().as_millis() as f32;
    let alpha = ((ms * 0.003_141_593).sin() * 0.3 + state.level).clamp(0.0, 1.0);
    for (mut img, mut v) in &mut nodes {
        img.color = Color::srgba(1.0, 1.0, 1.0, alpha);
        v.set_if_neq(Visibility::Inherited);
    }
}

#[derive(Component)]
struct LowHealthRoot;

#[derive(Component)]
struct LowHealthNode;

/// The low-health screen overlay (`InGameUI` frame `sub_14002BC00` / render `sub_140027350`):
/// `system/textures/lowhealth.tex` over the whole screen under the HUD, its level a pure
/// function of the life fraction `p`: 0 above `lowHealthEffectPercentage` (0.3) + 0.0125, `minOpacity` (0.7)
/// at the threshold - 0.0125 (linear between), up to 1 as life drops to 0; alpha is
/// `clamp(sin(t * 0.003141593 per ms) * 0.1 + level)`; only with the `screenEffects` option. PORT: the one-hit
/// flash drawn over the HUD (gated by an unresolved devotion-window query) is not drawn.
#[allow(clippy::too_many_arguments)]
fn low_health_overlay(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    clock: Res<Time<Real>>,
    values: Res<super::HudVitals>,
    roots: Query<Entity, With<LowHealthRoot>>,
    mut nodes: Query<(&mut ImageNode, &mut Visibility), With<LowHealthNode>>,
    mut built: Local<Option<UVec2>>,
    mut assets: UiAssets,
    options: Res<gd_game::options::GameOptions>,
) {
    if fresh.begun() {
        *built = None;
    }
    let (thr, min) = data.db.get(super::hud::MASTER).map_or((0.3, 0.7), |m| {
        (
            m.f32("lowHealthEffectPercentage").unwrap_or(0.3),
            m.f32("lowHealthEffectMinOpacity").unwrap_or(0.7),
        )
    });
    let p = if values.health_max > 0.0 {
        values.health / values.health_max
    } else {
        1.0
    };
    let band = 0.0125;
    let level = if p > thr + band {
        0.0
    } else if p > thr - band {
        (1.0 - (p - (thr - band)) / (2.0 * band)) * min
    } else {
        (1.0 - p / (thr - band)) * (1.0 - min) + min
    };
    if level <= 0.0 || values.health_max <= 0.0 || !options.flag("screenEffects") {
        for (_, mut v) in &mut nodes {
            v.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if *built != Some(size) || nodes.is_empty() {
        *built = Some(size);
        for e in &roots {
            commands.entity(e).try_despawn();
        }
        // Under the HUD (its roots are at the default layer).
        let root = crate::ui::spawn_root(&mut commands, LowHealthRoot, -1);
        let mut ui = assets.ui(
            &mut commands,
            &data,
            root,
            assets.scale(size),
            1.0 / win.scale_factor(),
            Vec2::new(size.x as f32, size.y as f32),
        );
        if let Some((image, _)) = ui.image("system/textures/lowhealth.tex") {
            let e = ui.quad_stretch(
                image,
                URect::new(0.0, 0.0, size.x as f32, size.y as f32),
                Color::srgba(1.0, 1.0, 1.0, 0.0),
            );
            ui.commands.entity(e).try_insert(LowHealthNode);
        }
        return;
    }
    let ms = clock.elapsed().as_millis() as f32;
    let alpha = ((ms * 0.003_141_593).sin() * 0.1 + level).clamp(0.0, 1.0);
    for (mut img, mut v) in &mut nodes {
        img.color = Color::srgba(1.0, 1.0, 1.0, alpha);
        v.set_if_neq(Visibility::Inherited);
    }
}

pub struct SurvivalUiPlugin;

impl Plugin for SurvivalUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurvivalView>()
            .init_resource::<EndlessView>()
            .init_resource::<GlowState>()
            .init_resource::<PaneRollovers>()
            .init_resource::<GlowOverlay>()
            .init_resource::<SunderedOverlay>()
            .init_resource::<MutatorView>()
            .add_systems(
                Update,
                (
                    (collect, collect_endless, collect_mutators),
                    (
                        detect_events,
                        detect_endless_events,
                        build_pane,
                        build_endless_pane,
                        build_mutators,
                        build_objectives,
                        animate_glows,
                        animate_warning,
                        fullscreen_glow,
                        sundered_overlay,
                        low_health_overlay,
                    )
                        .chain(),
                )
                    .chain()
                    .in_set(crate::ui::UiPlaySet),
            );
    }
}
