use super::*;
use gd_ui::state::options::Tab;

/// A page's columns of controls, in flow order (column-major). The coordinates are unset here; [`layout`]
/// measures the labels and places them. Identity-only callers (input, live, the open hook) use [`ctls`].
///
/// PORT: the engine's page builder (`OptionsWindow::Load`) walks a fixed row cursor with hard-coded column
/// offsets and skips rows for options the port does not show; the port lays each tab out as columns of
/// fixed-height rows instead, so an omitted option leaves no gap and a long label pushes its control right
/// rather than overflowing.
pub(super) fn page(tab: Tab) -> Vec<Vec<Ctl>> {
    fn check(k: &'static str, t: &'static str) -> Ctl {
        Ctl::Check(k, t, 0.0, 0.0)
    }
    fn check_left(k: &'static str, t: &'static str) -> Ctl {
        Ctl::CheckLeft(k, t, 0.0, 0.0, 0.0)
    }
    fn slider(k: &'static str, t: &'static str) -> Ctl {
        Ctl::Slider(k, t, 0.0, 0.0, 0.0, 0.0)
    }
    fn drop(s: Shown, t: &'static str) -> Ctl {
        Ctl::Drop(s, t, 0.0, 0.0, 0.0, 0.0)
    }
    match tab {
        Tab::Gameplay => vec![
            vec![
                check("classicCasting", "tagGameplayOption29"),
                check("targetLock", "tagGameplayOption20"),
                check("gamepadTargetLock", "tagGameplayOption31"),
                check("evadeFollowCursor", "tagGameplayOption36"),
                check("lockMapRotation", "tagGameplayOption15"),
                check("lockItemPickup", "tagGameplayOption11"),
                check("dayNightCycle", "tagGameplayOption34"),
                check("quickBuy", "tagGameplayOption02"),
                check("cloudSaving", "tagGameplayOption07"),
            ],
            vec![
                check("breakingMoveTo", "tagGameplayOption27"),
                check("cameraShake", "tagGameplayOption25"),
                check("gore", "tagGameplayOption16"),
                // (`tagGameplayOption17`, `blood`, only with more than one entry: its list gets one, so never shown.)
                slider("corpsePersistence", "tagGameplayOption19"),
                slider("inactiveUpdateRate", "tagGameplayOption23"),
                slider("autoLootRadius", "tagGameplayOption32"),
                drop(Shown::Language, "tagGameplayOption09"),
                drop(Shown::MainMenu, "tagHUDOption20"),
            ],
        ],
        Tab::Interface => vec![
            vec![
                check("tutorialTips", "tagGameplayOption18"),
                check("screenEffects", "tagHUDOption21"),
                check("errorMessages", "tagGameplayOption30"),
                check("extraRollovers", "tagGameplayOption05"),
                check("showMonsterLevelOnRollover", "tagGameplayOption06"),
                check("displayDamage", "tagGameplayOption04"),
                check("critMultipliers", "tagGameplayOption22"),
                check("autoItemTooltips", "tagGameplayOption03"),
                check("sortConfirmBypass", "tagHUDOption09"),
                check("newItemHighlight", "tagHUDOption22"),
                // PORT: the HUD FPS counter lives with the other interface toggles (the engine has no tag for it).
                check("showFPS", "Show FPS"),
            ],
            vec![
                check("targetOutline", "tagGameplayOption14"),
                check("showTime", "tagHUDOption19"),
                check("hotbarCooldownCounter", "tagHUDOption14"),
                check("toggledAurasSelf", "tagHUDOption15"),
                check("toggledAurasOther", "tagHUDOption16"),
                check("toggledWeaponEffects", "tagHUDOption17"),
                check("detailItemTooltips", "tagHUDOption23"),
                check("standardHUD", "tagHUDOption24"),
                check("transferConfirmBypass", "tagHUDOption25"),
                check("largeCursor", "tagHUDOption26"),
            ],
        ],
        Tab::HealthBars => vec![
            vec![
                check("playerLocalBar", "tagHUDOption06"),
                check("playerBars", "tagHUDOption01"),
                check("playerHealth", "tagHUDOption02"),
                check("petBars", "tagHUDOption07"),
                check("monsterBars", "tagHUDOption03"),
                check("monsterHealth", "tagHUDOption04"),
                check("monsterBarsUndamaged", "tagHUDOption08"),
                check("monsterIcons", "tagHUDOption05"),
            ],
            vec![
                check("healthBarsPercent", "tagHUDOption12"),
                check("playerBarsLarge", "tagHUDOption18"),
                check("monsterBarsLarge", "tagHUDOption10"),
                check("bossBarsLarge", "tagHUDOption11"),
                check("monsterBarsDebuffs", "tagHUDOption13"),
            ],
        ],
        Tab::Audio => vec![vec![
            drop(Shown::OutputDevice, "tagAudioOption07"),
            drop(Shown::InputDevice, "tagAudioOption09"),
            check("voicechatEnabled", "tagAudioOption11"),
            check("voicechatPushToTalk", "tagAudioOption13"),
            check("micLoopback", "tagAudioOption10"),
            slider("masterVolume", "tagAudioOption01"),
            slider("musicVolume", "tagAudioOption02"),
            slider("effectsVolume", "tagAudioOption03"),
            slider("ambientVolume", "tagAudioOption04"),
            slider("dialogVolume", "tagAudioOption05"),
            slider("voiceVolume", "tagAudioOption12"),
            check("errorSpeech", "tagAudioOption08"),
        ]],
        Tab::Video => vec![
            vec![
                drop(Shown::RenderDevice, "tagVideoOption30"),
                drop(Shown::WindowMode, "tagVideoOption29"),
                drop(Shown::Resolution, "tagVideoOption01"),
                // Anti-aliasing (the DirectX 11 deferred path): a check box with its label on the left.
                check_left("fxaa", "tagVideoOption13"),
                drop(Shown::Aniso, "tagVideoOption25"),
                drop(Shown::Quality("textureQuality"), "tagVideoOption02"),
                drop(Shown::Shadows, "tagVideoOption03"),
                slider("uiScale", "tagVideoOption23"),
                // "Fullscreen Gamma" (`gamma`): the exclusive fullscreen gamma ramp (gd-render `post.rs`).
                slider("gamma", "tagVideoOption10"),
            ],
            vec![
                drop(Shown::Quality("detailLevel"), "tagVideoOption24"),
                drop(Shown::Quality("reflectionQuality"), "tagVideoOption04"),
                drop(Shown::Quality("fxQuality"), "tagVideoOption27"),
                drop(Shown::Quality("lightingQuality"), "tagVideoOption28"),
                drop(Shown::Weather, "tagVideoOption26"),
                drop(Shown::ColorBlind, "tagVideoOption12"),
                check("syncToRefresh", "tagVideoOption11"),
                check("detailObjects", "tagVideoOption09"),
                check("ambientocclusion", "tagVideoOption15"),
                // PORT: the frame limiter (literal English label; the engine has no tag for it).
                drop(Shown::MaxFps, "FPS Limit"),
            ],
            vec![
                check("advancedEffects", "tagVideoOption08"),
                check("depthOfField", "tagVideoOption05"),
                check("softParticles", "tagVideoOption06"),
                check("hdrLighting", "tagVideoOption16"),
                check("fog", "tagVideoOption32"),
            ],
        ],
        // The key list fills the page above; these two sit in a row along its bottom edge.
        Tab::KeyBindings => vec![
            vec![check("gamepadSupport", "tagGameplayOption24S")],
            vec![drop(Shown::Movement, "tagGameplayOption35")],
        ],
        Tab::Network => vec![],
    }
}

/// The page's controls in flow order (column-major), coordinates unset: for callers that only read a control's
/// identity (the open dropdown's [`Shown`], the Default reset's option keys).
pub(super) fn ctls(tab: Tab) -> Vec<Ctl> {
    page(tab).into_iter().flatten().collect()
}

/// The layout environment `build` measures with: the page's content size (reference units, from the window
/// record's `pageExtent*`), the check-box and dropdown-arrow texture widths (reference units), and a closure
/// returning a label's measured width in reference units (`ui.text_width / ui.s`).
pub(super) struct Env<'a> {
    pub size: Vec2,
    pub check_w: f32,
    pub arrow_w: f32,
    pub measure: &'a mut dyn FnMut(&str) -> f32,
}

/// Side margin inside the page, gap between a label and its control, and the column gap range.
const PAD: f32 = 16.0;
const LABEL_GAP: f32 = 10.0;
const COL_GAP_MIN: f32 = 20.0;
const COL_GAP_MAX: f32 = 80.0;
/// The slider track's natural and minimum width, and the dropdown arrow's offset outside its box (reference
/// units).
const TRACK_W: f32 = 215.0;
const TRACK_MIN: f32 = 120.0;
const ARROW_GAP: f32 = 3.0;
/// The smallest a dropdown box may shrink to when the columns must fit a narrow page.
const DROP_MIN: f32 = 60.0;

/// A dropdown's box width (reference units) at shrink factor `f` (1 = natural): the audio device lists stay
/// wide, the FPS cap and quality lists shrink toward [`DROP_MIN`].
fn drop_bw(s: Shown, f: f32) -> f32 {
    let nat = match s {
        Shown::OutputDevice | Shown::InputDevice => return 400.0,
        Shown::MaxFps => 110.0,
        _ => 200.0,
    };
    DROP_MIN + (nat - DROP_MIN) * f
}

/// The slider track width at shrink factor `f`.
fn track_w(f: f32) -> f32 {
    TRACK_MIN + (TRACK_W - TRACK_MIN) * f
}

/// A tab's row height, top margin, and whether its rows sit along the page bottom (the key list's footer).
fn tab_metrics(tab: Tab) -> (f32, f32, bool) {
    match tab {
        Tab::Gameplay => (40.0, 15.0, false),
        Tab::Interface => (36.0, 15.0, false),
        Tab::HealthBars => (40.0, 15.0, false),
        Tab::Audio => (33.0, 15.0, false),
        Tab::Video => (35.0, 20.0, false),
        Tab::KeyBindings => (35.0, 0.0, true),
        Tab::Network => (35.0, 0.0, false),
    }
}

/// Place a tab's controls: measure each column's widest left label, size the columns to their content, spread
/// them across the page (shrinking dropdown boxes when they would not fit), and set every control just past its
/// label, clamped so the box and its arrow stay inside the page's right edge. Returns the controls in flow order
/// with page-local reference coordinates.
pub(super) fn layout(tab: Tab, env: &mut Env) -> Vec<Ctl> {
    let mut cols = page(tab);
    let (row_h, top, bottom) = tab_metrics(tab);
    let n = cols.len();
    let usable = env.size.x - 2.0 * PAD;
    // The widest left label per column (a check box's label sits right of it, so does not push the column).
    let mut label_max = vec![0.0f32; n];
    for (i, col) in cols.iter().enumerate() {
        for c in col {
            let lw = match *c {
                Ctl::CheckLeft(_, t, ..) | Ctl::Slider(_, t, ..) | Ctl::Drop(_, t, ..) => {
                    (env.measure)(t)
                }
                _ => 0.0,
            };
            label_max[i] = label_max[i].max(lw);
        }
    }
    // A row's width at shrink factor `f` (dropdown boxes and slider tracks shrink together).
    let mut row_w = |c: &Ctl, lm: f32, f: f32| -> f32 {
        match *c {
            Ctl::Check(_, t, ..) => env.check_w + LABEL_GAP + (env.measure)(t),
            Ctl::CheckLeft(..) => lm + LABEL_GAP + env.check_w,
            Ctl::Slider(..) => lm + LABEL_GAP + track_w(f),
            Ctl::Drop(s, ..) => lm + LABEL_GAP + drop_bw(s, f) + ARROW_GAP + env.arrow_w,
        }
    };
    let mut col_w = |col: &[Ctl], lm: f32, f: f32| -> f32 {
        col.iter().map(|c| row_w(c, lm, f)).fold(0.0, f32::max)
    };
    let mut widths_at =
        |f: f32| -> Vec<f32> { (0..n).map(|i| col_w(&cols[i], label_max[i], f)).collect() };
    let fits = |w: &[f32]| -> bool {
        w.iter().sum::<f32>() + COL_GAP_MIN * n.saturating_sub(1) as f32 <= usable
    };
    // The largest shrink factor whose columns fit the page (1 = natural widths; binary search down to 0).
    let mut f = 1.0;
    if n > 1 && !fits(&widths_at(f)) {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..24 {
            let mid = (lo + hi) * 0.5;
            if fits(&widths_at(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        f = lo;
    }
    let widths = widths_at(f);
    let total: f32 = widths.iter().sum();
    let gap = if n > 1 {
        ((usable - total) / (n - 1) as f32).clamp(COL_GAP_MIN, COL_GAP_MAX)
    } else {
        0.0
    };
    let mut x = PAD;
    for (i, col) in cols.iter_mut().enumerate() {
        let col_right = (x + widths[i]).min(env.size.x - PAD);
        let lm = label_max[i];
        let base_y = if bottom {
            env.size.y - row_h - 8.0
        } else {
            top
        };
        for (r, c) in col.iter_mut().enumerate() {
            let cy = base_y + row_h * r as f32 + row_h * 0.5;
            let ctrl_x = x + lm + LABEL_GAP;
            match c {
                Ctl::Check(_, _, bx, by) => {
                    *bx = x;
                    *by = cy - 12.0;
                }
                Ctl::CheckLeft(_, _, lx, lcy, bx) => {
                    *lx = x;
                    *lcy = cy;
                    *bx = ctrl_x.min(col_right - env.check_w).max(x);
                }
                Ctl::Slider(_, _, lx, lcy, x0, x1) => {
                    *lx = x;
                    *lcy = cy;
                    let tw = track_w(f);
                    let a = ctrl_x.min(col_right - tw).max(x);
                    *x0 = a;
                    *x1 = a + tw;
                }
                Ctl::Drop(s, _, lx, lcy, x0, x1) => {
                    *lx = x;
                    *lcy = cy;
                    let bw = drop_bw(*s, f);
                    let a = ctrl_x.min(col_right - bw - ARROW_GAP - env.arrow_w).max(x);
                    let bw = (col_right - a - ARROW_GAP - env.arrow_w).min(bw).max(20.0);
                    *x0 = a;
                    *x1 = a + bw;
                }
            }
        }
        x += widths[i] + gap;
    }
    cols.into_iter().flatten().collect()
}

/// The environment a dropdown's rows / shown text read besides the draft: the audio device lists, the render
/// adapter list, the mounted localisations, the effective primary monitor
/// ([`crate::settings::primary_monitor`], its video modes and physical size), and the localisation tag table
/// ([`gd_formats::text::TagTable`]) the value labels resolve through. `screen` is the window's physical
/// size, the resolution fallback when there is no monitor. Built once per calling system; the monitor's modes
/// are session-stable.
pub(super) struct ChoiceCtx<'a> {
    devices: &'a gd_audio::devices::AudioDevices,
    pub adapters: &'a crate::render_devices::RenderAdapters,
    pub languages: &'a [&'static gd_formats::text::Language],
    pub tags: &'a gd_formats::text::TagTable,
    monitor: Option<&'a bevy::window::Monitor>,
    screen: Vec2,
}

impl<'a> ChoiceCtx<'a> {
    pub(super) fn new(
        devices: &'a gd_audio::devices::AudioDevices,
        monitor: Option<&'a bevy::window::Monitor>,
        screen: Vec2,
        adapters: &'a crate::render_devices::RenderAdapters,
        languages: &'a [&'static gd_formats::text::Language],
        tags: &'a gd_formats::text::TagTable,
    ) -> Self {
        Self {
            devices,
            adapters,
            languages,
            tags,
            monitor,
            screen,
        }
    }
}

/// A device dropdown's rows (`OptionsWindow::Load` enumerates the host's devices when the window opens): row 0
/// "Default" sets index 0 (the system default, not a listed entry), row k + 1 sets the list's entry k.
fn device_rows(
    k: &'static str,
    entries: &[gd_audio::devices::AudioDeviceEntry],
) -> Vec<(String, Vec<(&'static str, Value)>)> {
    std::iter::once(("Default".to_string(), vec![(k, Value::Int(0))]))
        .chain(
            entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.name.clone(), vec![(k, Value::Int(i as i32 + 1))])),
        )
        .collect()
}

/// The device a stored index shows: 0 / out of range → "Default", else the entry's name.
fn device_shown(index: i32, entries: &[gd_audio::devices::AudioDeviceEntry]) -> String {
    index
        .checked_sub(1)
        .and_then(|i| entries.get(i as usize))
        .map_or_else(|| "Default".to_string(), |e| e.name.clone())
}

/// The stored `language` option string (the engine's spelling, e.g. "English"); the factory default when the
/// value is missing or not a string.
pub(super) fn language_option(o: &GameOptions) -> &str {
    match o.get("language") {
        Some(Value::Str(s)) => s,
        _ => "English",
    }
}

/// A render adapter's row label: "[i] name (Backend)" — 1-based display, the stored `device` index is 0-based.
fn adapter_label(i: usize, e: &crate::render_devices::RenderAdapter) -> String {
    format!("[{}] {} ({})", i + 1, e.name, e.backend)
}

/// The aspect suffix the engine appends to a resolution label (`OptionsWindow::AddResolutionDropBox`): the
/// gcd-reduced ratio when it is one of 16:9 / 16:10 / 21:9 / 32:9.
fn aspect_suffix(w: u32, h: u32) -> &'static str {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    if h == 0 {
        return "";
    }
    let g = gcd(w, h);
    match (w / g, h / g) {
        (16, 9) => " (16:9)",
        (16, 10) => " (16:10)",
        (21, 9) => " (21:9)",
        (32, 9) => " (32:9)",
        _ => "",
    }
}

/// A mode's row label: "{w} x {h} @{hz}hz" + the aspect suffix (`OptionsWindow::AddResolutionDropBox`).
fn mode_label(w: u32, h: u32, millihertz: u32) -> String {
    format!(
        "{} x {} @{}hz{}",
        w,
        h,
        millihertz / 1000,
        aspect_suffix(w, h)
    )
}

/// The stored `resolution` when it passes the window-size clamp ([`crate::settings::window_size`]).
fn stored_resolution(o: &GameOptions) -> Option<(u32, u32)> {
    match o.get("resolution") {
        Some(Value::Vec2(w, h)) => crate::settings::window_size(*w, *h),
        _ => None,
    }
}

/// The resolution dropdown's rows (`OptionsWindow::AddResolutionDropBox`, the mode list `OptionsWindow::Load`
/// fills from the graphics engine's supported modes): the effective primary monitor's modes at least 1024 x 720, labelled
/// and deduped, ascending by (w, h); when the stored resolution is valid but no mode row carries it, the window's
/// physical size is appended as the last row.
/// PORT: a row writes `resolution` only — the engine also writes `refreshRate`, which the port does not apply
/// (vsync is `syncToRefresh`).
/// PORT: the list is exactly what the monitor reports, with no synthetic entries — Wayland compositors commonly
/// expose only the current mode (a platform limitation).
fn resolution_rows(o: &GameOptions, ctx: &ChoiceCtx) -> Vec<(String, Vec<(&'static str, Value)>)> {
    let mut rows: Vec<(String, Vec<(&'static str, Value)>)> = Vec::new();
    if let Some(m) = ctx.monitor {
        let mut modes: Vec<(u32, u32, u32)> = m
            .video_modes
            .iter()
            .filter(|v| v.physical_size.x >= 1024 && v.physical_size.y >= 720)
            .map(|v| {
                (
                    v.physical_size.x,
                    v.physical_size.y,
                    v.refresh_rate_millihertz,
                )
            })
            .collect();
        modes.sort_by_key(|&(w, h, _)| (w, h));
        for (w, h, hz) in modes {
            let label = mode_label(w, h, hz);
            if rows.iter().any(|(l, _)| *l == label) {
                continue;
            }
            rows.push((label, vec![("resolution", Value::Vec2(w as i32, h as i32))]));
        }
    }
    let carries = |rows: &[(String, Vec<(&'static str, Value)>)], w: u32, h: u32| {
        rows.iter().any(|(_, sets)| {
            matches!(sets.first(), Some((_, Value::Vec2(rw, rh))) if (*rw as u32, *rh as u32) == (w, h))
        })
    };
    if let Some((sw, sh)) = stored_resolution(o)
        && !carries(&rows, sw, sh)
    {
        let (w, h) = (ctx.screen.x as u32, ctx.screen.y as u32);
        rows.push((
            format!("{} x {}", w, h),
            vec![("resolution", Value::Vec2(w as i32, h as i32))],
        ));
    }
    rows
}

/// The resolution the dropdown shows: the stored option when valid, else the effective auto size (the effective
/// primary monitor's physical size, else the window's) — never the live window size alone. Shown as the matching row's
/// label when a row carries that size, else "{w} x {h}".
fn resolution_shown(o: &GameOptions, ctx: &ChoiceCtx) -> String {
    let (w, h) = stored_resolution(o).unwrap_or_else(|| {
        ctx.monitor
            .map(|m| m.physical_size())
            .map(|s| (s.x, s.y))
            .unwrap_or((ctx.screen.x as u32, ctx.screen.y as u32))
    });
    resolution_rows(o, ctx)
        .into_iter()
        .find(|(_, sets)| {
            matches!(sets.first(), Some((_, Value::Vec2(rw, rh))) if (*rw as u32, *rh as u32) == (w, h))
        })
        .map(|(label, _)| label)
        .unwrap_or_else(|| format!("{} x {}", w, h))
}

/// Movement-type rows (`OptionsWindow::Load`): (display tag, English fallback) in `movementType` order.
const MOVEMENT: [(&str, &str); 3] = [
    ("tagMovementClassic", "Classic"),
    ("tagMovementKeyboard", "Keyboard"),
    ("tagMovementKeyboardOnly", "Keyboard Only"),
];

/// Window-mode rows (`OptionsWindow::AddWindowModeDropBox`): (display tag, English fallback) in `screenMode`
/// order.
const WINDOW_MODE: [(&str, &str); 3] = [
    ("tagFullscreen", "Fullscreen"),
    ("tagWindowed", "Windowed"),
    ("tagFullscreenWindowed", "Windowed (Borderless)"),
];

/// Color-blind rows (`OptionsWindow::Load`): (display tag, English fallback) in `colorblind` order.
const COLOR_BLIND: [(&str, &str); 4] = [
    ("tagNormal", "Normal"),
    ("tagProtanopia", "Protanopia"),
    ("tagDeuteranopia", "Deuteranopia"),
    ("tagTritanopia", "Tritanopia"),
];

/// A dropdown value's localized label: the tag's text (`LocalizationManager::Localize`), the English fallback
/// when the tag is missing. [`crate::ui::text::FontCache::tag`]'s plain lookup — [`gd_formats::text::TagTable`]
/// strips the `{}` param braces at load, so a no-param tag needs no further handling.
fn label(ctx: &ChoiceCtx, key: &str, fallback: &str) -> String {
    ctx.tags.get(key).unwrap_or(fallback).to_string()
}

/// The value a closed dropdown box shows. The movement / window-mode / color-blind / quality / Off labels
/// localize through the engine tags (`OptionsWindow::Load`, `LocalizationManager::Localize`) like the open rows
/// ([`choices`]); the MainMenu product names, the language display names and the host's device / resolution /
/// render-device labels stay invariant.
pub(super) fn shown_text(s: Shown, o: &GameOptions, ctx: &ChoiceCtx) -> String {
    match s {
        // The stored option string's display name (`gd_formats::text::LANGUAGES`); an unknown spelling shows
        // the factory default.
        Shown::Language => gd_formats::text::LANGUAGES
            .iter()
            .find(|l| l.option == language_option(o))
            .map_or("English (US)", |l| l.name)
            .into(),
        Shown::MainMenu => [
            "Grim Dawn",
            "Ashes of Malmouth",
            "Forgotten Gods",
            "Fangs of Asterkarn",
        ]
        .get(o.int("mainMenu").clamp(0, 3) as usize)
        .copied()
        .unwrap_or_default()
        .into(),
        Shown::OutputDevice => device_shown(o.int("soundDevice"), &ctx.devices.output),
        Shown::InputDevice => device_shown(o.int("captureDevice"), &ctx.devices.input),
        // The stored `device` index clamped to the adapter list (out of range → row 0); no adapter
        // enumerated (headless) keeps the placeholder.
        Shown::RenderDevice => {
            let entries = &ctx.adapters.entries;
            if entries.is_empty() {
                "[1] wgpu".into()
            } else {
                let i = o.int("device").clamp(0, entries.len() as i32 - 1) as usize;
                adapter_label(i, &entries[i])
            }
        }
        // `OptionsWindow::AddWindowModeDropBox`: 0 Fullscreen, 1 Windowed, 2 Windowed (Borderless) — the order
        // [`crate::settings::window_mode`] applies.
        Shown::WindowMode => {
            let (tag, en) = WINDOW_MODE[o.int("screenMode").clamp(0, 2) as usize];
            label(ctx, tag, en)
        }
        Shown::Resolution => resolution_shown(o, ctx),
        Shown::Aniso => match o.int("anisotropicFiltering") {
            n if n <= 1 => label(ctx, "tagOptionOff", "Off"),
            n => format!("{n}x"),
        },
        Shown::Quality(_) | Shown::Weather | Shown::Shadows => {
            let i = selected(s, o);
            // The list's entry (the engine shows the selected item's text), localized like the rows.
            let (tag, en) = match s {
                Shown::Quality(_) => (QUALITY[i].0, quality_english(QUALITY[i].0)),
                _ if i == 0 => ("tagOptionOff", "Off"),
                _ => (QUALITY[i - 1].0, quality_english(QUALITY[i - 1].0)),
            };
            label(ctx, tag, en)
        }
        Shown::ColorBlind => {
            let (tag, en) = COLOR_BLIND[o.int("colorblind").clamp(0, 3) as usize];
            label(ctx, tag, en)
        }
        Shown::Movement => {
            let (tag, en) = MOVEMENT[o.int("movementType").clamp(0, 2) as usize];
            label(ctx, tag, en)
        }
        // PORT: the frame-limiter cap; a value no row carries shows as its number (like a resolution).
        Shown::MaxFps => {
            let n = o.int("maxFPS");
            if n <= 0 {
                label(ctx, "tagOptionOff", "Off")
            } else {
                format!("{n}")
            }
        }
    }
}

/// Quality display tags (`Options::PrintToConsole`, Engine.dll) and their file spelling; the display localizes
/// via [`label`] with the [`quality_english`] fallback.
pub(super) const QUALITY: [(&str, &str); 5] = [
    ("tagVideoLevel03", "low"),
    ("tagVideoLevel02", "medium"),
    ("tagVideoLevel01", "high"),
    ("tagVideoLevel04", "very high"),
    ("tagVideoLevel06", "ultra"),
];

/// PORT: the `maxFPS` dropdown's caps, after the leading Off (0, uncapped).
const FPS_CAPS: [i32; 5] = [30, 60, 120, 144, 240];

/// The English quality names [`QUALITY`]'s display tags localize (`Options::PrintToConsole`, Engine.dll): the
/// fallback when a tag is missing (an unknown tag keeps the file default's name, medium).
fn quality_english(tag: &str) -> &'static str {
    match tag {
        "tagVideoLevel03" => "Low",
        "tagVideoLevel02" => "Medium",
        "tagVideoLevel01" => "High",
        "tagVideoLevel04" => "Very High",
        "tagVideoLevel06" => "Ultra",
        _ => "Medium",
    }
}

/// Number of quality entries a dropdown offers (without the leading Off of weather / shadows). Jump table of
/// `OptionsWindow::AddQualityDropBox`, pick handler `OptionsWindow::HandleVideoPick`, page builder
/// `OptionsWindow::Load`: textureQuality Low..High (3); fxQuality, lightingQuality and weather Low..Very High (4);
/// detailLevel (5); shadows Low..Ultra (5) after Off.
pub(super) fn level_count(s: Shown) -> usize {
    match s {
        Shown::Quality("textureQuality") => 3,
        Shown::Quality("detailLevel") | Shown::Shadows => 5,
        _ => 4,
    }
}

/// The first `n` [`QUALITY`] entries (display tag, file spelling), mapped to rows.
pub(super) fn levels<T>(n: usize, f: impl Fn((&'static str, &'static str)) -> T) -> Vec<T> {
    QUALITY[..n].iter().copied().map(f).collect()
}

/// The quality in the file as an index. `Options::LoadFile`: "low" 0, "high" 2, "very high" 3,
/// "ultra" 4, anything else 1 (medium).
pub(super) fn quality_level(o: &GameOptions, k: &str) -> usize {
    match o.get(k) {
        Some(Value::Quality(q)) => QUALITY.iter().position(|(_, v)| v == q).unwrap_or(1),
        _ => 1,
    }
}

/// The list item a dropdown shows as selected. `OptionsWindow::Load`: the stored quality index clamped to the entry
/// count (a file's "ultra" texture quality shows as High, the file value is kept until a pick); weather: item 0 when
/// `weatherEnabled` is off, else quality + 1; shadows: item 0 when `shadows` is off, else `shadowQuality` + 1.
pub(super) fn selected(s: Shown, o: &GameOptions) -> usize {
    let max = level_count(s) - 1;
    match s {
        Shown::MaxFps => FPS_CAPS
            .iter()
            .position(|&n| n == o.int("maxFPS"))
            .map_or(0, |i| i + 1),
        Shown::Quality(k) => quality_level(o, k).min(max),
        Shown::Weather if !o.flag("weatherEnabled") => 0,
        Shown::Weather => quality_level(o, "weatherQuality").min(max) + 1,
        Shown::Shadows if !o.flag("shadows") => 0,
        Shown::Shadows => quality_level(o, "shadowQuality").min(max) + 1,
        _ => 0,
    }
}

/// A dropdown's rows: (text, the options a pick sets). The device / resolution / window-mode lists are the
/// engine's (`OptionsWindow::Load`, `OptionsWindow::AddWindowModeDropBox`, `OptionsWindow::AddResolutionDropBox`);
/// the render-device list is the host's wgpu adapters (`crate::render_devices`: 0-based `device` index, no
/// Default row — the engine's device index selects a listed adapter directly) and the language list the mounted
/// localisations (`GameData::available_languages`); the quality lists are the engine's (see [`level_count`];
/// the file spells qualities in lower case). The movement / window-mode / color-blind / quality / Off row texts
/// localize through the engine tags (`OptionsWindow::Load`, `LocalizationManager::Localize`) — the same labels
/// [`shown_text`] shows, so the selection matching holds; the stored values stay the engine's.
pub(super) fn choices(
    s: Shown,
    o: &GameOptions,
    ctx: &ChoiceCtx,
) -> Vec<(String, Vec<(&'static str, Value)>)> {
    let ints = |k: &'static str, names: &[&str]| {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), vec![(k, Value::Int(i as i32))]))
            .collect()
    };
    // The localized integer rows: (display tag, English fallback) pairs; the pick still sets the row's index.
    let tagged_ints = |k: &'static str, rows: &[(&str, &str)]| {
        rows.iter()
            .enumerate()
            .map(|(i, &(tag, en))| (label(ctx, tag, en), vec![(k, Value::Int(i as i32))]))
            .collect()
    };
    match s {
        Shown::MainMenu => ints(
            "mainMenu",
            &[
                "Grim Dawn",
                "Ashes of Malmouth",
                "Forgotten Gods",
                "Fangs of Asterkarn",
            ],
        ),
        Shown::WindowMode => tagged_ints("screenMode", &WINDOW_MODE),
        Shown::ColorBlind => tagged_ints("colorblind", &COLOR_BLIND),
        Shown::Movement => tagged_ints("movementType", &MOVEMENT),
        Shown::MaxFps => std::iter::once((
            label(ctx, "tagOptionOff", "Off"),
            vec![("maxFPS", Value::Int(0))],
        ))
        .chain(
            FPS_CAPS
                .iter()
                .map(|&n| (format!("{n}"), vec![("maxFPS", Value::Int(n))])),
        )
        .collect(),
        Shown::Aniso => [1, 2, 4, 8, 16]
            .into_iter()
            .map(|n| {
                (
                    if n == 1 {
                        label(ctx, "tagOptionOff", "Off")
                    } else {
                        format!("{n}x")
                    },
                    vec![("anisotropicFiltering", Value::Int(n))],
                )
            })
            .collect(),
        Shown::Quality(k) => levels(level_count(s), |(tag, v)| {
            (
                label(ctx, tag, quality_english(tag)),
                vec![(k, Value::Quality(v.to_string()))],
            )
        }),
        Shown::Weather => std::iter::once((
            label(ctx, "tagOptionOff", "Off"),
            vec![("weatherEnabled", Value::Bool(false))],
        ))
        .chain(levels(level_count(s), |(tag, v)| {
            (
                label(ctx, tag, quality_english(tag)),
                vec![
                    ("weatherEnabled", Value::Bool(true)),
                    ("weatherQuality", Value::Quality(v.to_string())),
                ],
            )
        }))
        .collect(),
        Shown::Shadows => std::iter::once((
            label(ctx, "tagOptionOff", "Off"),
            vec![("shadows", Value::Bool(false))],
        ))
        .chain(levels(level_count(s), |(tag, v)| {
            (
                label(ctx, tag, quality_english(tag)),
                vec![
                    ("shadows", Value::Bool(true)),
                    ("shadowQuality", Value::Quality(v.to_string())),
                ],
            )
        }))
        .collect(),
        Shown::OutputDevice => device_rows("soundDevice", &ctx.devices.output),
        // PORT: `captureDevice` is kept for parity (voice chat is unported); the engine binds this list to
        // `soundDevice` (an `OptionsWindow::Load` bug) — not replicated.
        Shown::InputDevice => device_rows("captureDevice", &ctx.devices.input),
        Shown::Resolution => resolution_rows(o, ctx),
        Shown::RenderDevice => ctx
            .adapters
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (adapter_label(i, e), vec![("device", Value::Int(i as i32))]))
            .collect(),
        Shown::Language => ctx
            .languages
            .iter()
            .map(|l| {
                (
                    l.name.to_string(),
                    vec![("language", Value::Str(l.option.to_string()))],
                )
            })
            .collect(),
    }
}

/// The knob's bitmap index into [`KNOB`] (the slider's draw `MenuSlider::Draw`): held while dragged, else lit
/// while the cursor is on the knob, else up.
pub(super) fn knob_bitmap(dragged: bool, hovered: bool) -> usize {
    match (dragged, hovered) {
        (true, _) => 1,
        (false, true) => 2,
        _ => 0,
    }
}

/// The rollover text tag of the control labelled `label` (the page builders `OptionsWindow::Load`,
/// `OptionsWindow::AddBloodDropBox`, `OptionsWindow::AddWindowModeDropBox`, `OptionsWindow::AddResolutionDropBox`: each
/// check box / slider / dropdown gets its label's `tag<Page>Option<NN>` with
/// `Option` → `Desc`, except the microphone list and the three voice-chat boxes, whose rollover repeats the label).
pub(super) fn rollover_tag(label: &str) -> String {
    match label {
        "tagAudioOption09" | "tagAudioOption10" | "tagAudioOption11" | "tagAudioOption13" => {
            label.to_string()
        }
        _ => label.replacen("Option", "Desc", 1),
    }
}

/// A slider option's value as 0..1 and back (volumes, UI scale are
/// 0..1 already; PORT: corpse persistence / background FPS / auto loot
/// radius are taken as 0..1 too).
pub(super) fn slider_get(o: &GameOptions, k: &str) -> f32 {
    o.float(k).clamp(0.0, 1.0)
}
