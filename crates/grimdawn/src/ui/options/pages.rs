use super::*;
use gd_ui::state::options::Tab;

/// The page origin in the reference shots' coordinates: [`ORIGIN`] + `pageX` / `pageY` (15, 114).
pub(super) const PAGE: Vec2 = Vec2::new(ORIGIN.x + 15.0, ORIGIN.y + 114.0);

/// A page under construction (builders of `OptionsWindow::Load`): `y` is the page's row
/// cursor (`OptionsPage.rowY`). A check box (`OptionsWindow::AddCheckBox`: x, dy) sits at (x + 20, y), then y += dy; a
/// slider (`OptionsWindow::AddSlider`) is a 215 x 20 track at (x, y), its label 181 px left of it, then y += 35; a
/// dropdown (`OptionsWindow::AddQualityDropBox` / `AddIntDropBox` / `AddListDropBox` / `AddAnisotropyDropBox`: label
/// x, box x, width; 200 when not given) is a width x 23 box at (box x, y), its label at (label x, y), then y += 35. A
/// new page's cursor is 20 (page ctor `OptionsPage::OptionsPage`).
pub(super) struct Page {
    y: f32,
    ctls: Vec<Ctl>,
}

impl Page {
    fn new(y: f32) -> Self {
        Self {
            y,
            ctls: Vec::new(),
        }
    }

    fn check(&mut self, o: &'static str, t: &'static str, x: f32, dy: f32) {
        self.ctls
            .push(Ctl::Check(o, t, PAGE.x + 20.0 + x, PAGE.y + self.y));
        self.y += dy;
    }

    fn checks(&mut self, opts: &[(&'static str, &'static str)], x: f32, dy: f32) {
        for (o, t) in opts {
            self.check(o, t, x, dy);
        }
    }

    fn slider(&mut self, o: &'static str, t: &'static str, x: f32) {
        let x = PAGE.x + x;
        self.ctls.push(Ctl::Slider(
            o,
            t,
            x - 181.0,
            PAGE.y + self.y + 10.0,
            x,
            x + 215.0,
        ));
        self.y += 35.0;
    }

    fn drop(&mut self, s: Shown, t: &'static str, label_x: f32, box_x: f32, width: f32) {
        let x = PAGE.x + box_x;
        self.ctls.push(Ctl::Drop(
            s,
            t,
            PAGE.x + label_x,
            PAGE.y + self.y + 11.5,
            x,
            x + width - 3.0,
        ));
        self.y += 35.0;
    }
}

pub(super) fn page(tab: Tab) -> Vec<Ctl> {
    let mut p = Page::new(20.0);
    match tab {
        Tab::Gameplay => {
            p.y = 15.0;
            p.checks(
                &[
                    ("classicCasting", "tagGameplayOption29"),
                    ("targetLock", "tagGameplayOption20"),
                    ("gamepadTargetLock", "tagGameplayOption31"),
                    ("evadeFollowCursor", "tagGameplayOption36"),
                ],
                0.0,
                40.0,
            );
            p.y += 20.0;
            p.checks(
                &[
                    ("lockMapRotation", "tagGameplayOption15"),
                    ("lockItemPickup", "tagGameplayOption11"),
                    ("dayNightCycle", "tagGameplayOption34"),
                    ("quickBuy", "tagGameplayOption02"),
                ],
                0.0,
                40.0,
            );
            p.y += 20.0;
            p.check("cloudSaving", "tagGameplayOption07", 0.0, 40.0);
            p.y = 20.0;
            p.checks(
                &[
                    ("breakingMoveTo", "tagGameplayOption27"),
                    ("cameraShake", "tagGameplayOption25"),
                    ("gore", "tagGameplayOption16"),
                ],
                340.0,
                40.0,
            );
            p.y += 20.0;
            // (`tagGameplayOption17`, `blood`, only with more than one entry: its list gets one, so never shown.)
            p.slider("corpsePersistence", "tagGameplayOption19", 540.0);
            p.slider("inactiveUpdateRate", "tagGameplayOption23", 540.0);
            p.slider("autoLootRadius", "tagGameplayOption32", 540.0);
            p.drop(Shown::Language, "tagGameplayOption09", 360.0, 540.0, 200.0);
            p.drop(Shown::MainMenu, "tagHUDOption20", 360.0, 540.0, 200.0);
        }
        Tab::Interface => {
            p.y = 15.0;
            p.checks(
                &[
                    ("tutorialTips", "tagGameplayOption18"),
                    ("screenEffects", "tagHUDOption21"),
                    ("errorMessages", "tagGameplayOption30"),
                    ("extraRollovers", "tagGameplayOption05"),
                    ("showMonsterLevelOnRollover", "tagGameplayOption06"),
                    ("displayDamage", "tagGameplayOption04"),
                    ("critMultipliers", "tagGameplayOption22"),
                    ("autoItemTooltips", "tagGameplayOption03"),
                    ("sortConfirmBypass", "tagHUDOption09"),
                    ("newItemHighlight", "tagHUDOption22"),
                ],
                0.0,
                40.0,
            );
            p.y = 15.0;
            p.checks(
                &[
                    ("targetOutline", "tagGameplayOption14"),
                    ("showTime", "tagHUDOption19"),
                    ("hotbarCooldownCounter", "tagHUDOption14"),
                    ("toggledAurasSelf", "tagHUDOption15"),
                    ("toggledAurasOther", "tagHUDOption16"),
                    ("toggledWeaponEffects", "tagHUDOption17"),
                    ("detailItemTooltips", "tagHUDOption23"),
                    ("standardHUD", "tagHUDOption24"),
                    ("transferConfirmBypass", "tagHUDOption25"),
                    ("largeCursor", "tagHUDOption26"),
                ],
                400.0,
                40.0,
            );
        }
        Tab::HealthBars => {
            p.y = 15.0;
            p.checks(
                &[
                    ("playerLocalBar", "tagHUDOption06"),
                    ("playerBars", "tagHUDOption01"),
                    ("playerHealth", "tagHUDOption02"),
                    ("petBars", "tagHUDOption07"),
                    ("monsterBars", "tagHUDOption03"),
                    ("monsterHealth", "tagHUDOption04"),
                    ("monsterBarsUndamaged", "tagHUDOption08"),
                    ("monsterIcons", "tagHUDOption05"),
                ],
                0.0,
                40.0,
            );
            p.y = 15.0;
            p.checks(
                &[
                    ("healthBarsPercent", "tagHUDOption12"),
                    ("playerBarsLarge", "tagHUDOption18"),
                    ("monsterBarsLarge", "tagHUDOption10"),
                    ("bossBarsLarge", "tagHUDOption11"),
                    ("monsterBarsDebuffs", "tagHUDOption13"),
                ],
                400.0,
                40.0,
            );
        }
        Tab::Audio => {
            p.drop(Shown::OutputDevice, "tagAudioOption07", 20.0, 200.0, 400.0);
            p.drop(Shown::InputDevice, "tagAudioOption09", 20.0, 200.0, 400.0);
            p.y += 10.0;
            p.check("voicechatEnabled", "tagAudioOption11", 0.0, 40.0);
            p.y = 100.0;
            p.check("voicechatPushToTalk", "tagAudioOption13", 150.0, 40.0);
            p.y = 100.0;
            p.check("micLoopback", "tagAudioOption10", 350.0, 40.0);
            p.y += 5.0;
            for (o, t) in [
                ("masterVolume", "tagAudioOption01"),
                ("musicVolume", "tagAudioOption02"),
                ("effectsVolume", "tagAudioOption03"),
                ("ambientVolume", "tagAudioOption04"),
                ("dialogVolume", "tagAudioOption05"),
                ("voiceVolume", "tagAudioOption12"),
            ] {
                p.slider(o, t, 200.0);
            }
            p.y += 5.0;
            p.check("errorSpeech", "tagAudioOption08", 0.0, 40.0);
        }
        Tab::Video => {
            p.drop(Shown::RenderDevice, "tagVideoOption30", 20.0, 200.0, 200.0);
            p.drop(Shown::WindowMode, "tagVideoOption29", 20.0, 200.0, 200.0);
            p.drop(Shown::Resolution, "tagVideoOption01", 20.0, 200.0, 200.0);
            // Anti-aliasing (the DirectX 11 deferred path: a check box, `fxaa`, label on its left: [`loose_labels`]).
            p.ctls.push(Ctl::Check("fxaa", "", 735.0, PAGE.y + p.y));
            p.y += 40.0;
            p.drop(Shown::Aniso, "tagVideoOption25", 20.0, 200.0, 200.0);
            p.drop(
                Shown::Quality("textureQuality"),
                "tagVideoOption02",
                20.0,
                200.0,
                200.0,
            );
            p.drop(Shown::Shadows, "tagVideoOption03", 20.0, 200.0, 200.0);
            p.y += 30.0; // `deferredrendering` (tagVideoOption31), not shown.
            p.y += 20.0;
            p.slider("uiScale", "tagVideoOption23", 200.0);
            // "Fullscreen Gamma" (`gamma`; page builder `OptionsWindow::Load` → `OptionsWindow::AddSlider`): the
            // exclusive fullscreen gamma ramp (gd-render `post.rs` `with_gamma_ramp`).
            p.slider("gamma", "tagVideoOption10", 200.0);
            p.y = 20.0;
            p.drop(
                Shown::Quality("detailLevel"),
                "tagVideoOption24",
                460.0,
                600.0,
                200.0,
            );
            p.drop(
                Shown::Quality("reflectionQuality"),
                "tagVideoOption04",
                460.0,
                600.0,
                200.0,
            );
            p.drop(
                Shown::Quality("fxQuality"),
                "tagVideoOption27",
                460.0,
                600.0,
                200.0,
            );
            p.drop(
                Shown::Quality("lightingQuality"),
                "tagVideoOption28",
                460.0,
                600.0,
                200.0,
            );
            p.drop(Shown::Weather, "tagVideoOption26", 460.0, 600.0, 200.0);
            p.drop(Shown::ColorBlind, "tagVideoOption12", 460.0, 600.0, 200.0);
            p.y = 240.0;
            p.check("syncToRefresh", "tagVideoOption11", 440.0, 30.0);
            p.y += 30.0; // `tripleBuffer` (tagVideoOption07), not shown.
            p.check("detailObjects", "tagVideoOption09", 440.0, 30.0);
            p.y += 30.0; // `alphatocoverage` (tagVideoOption14), not shown.
            p.check("ambientocclusion", "tagVideoOption15", 440.0, 30.0);
            p.y = 240.0;
            p.checks(
                &[
                    ("advancedEffects", "tagVideoOption08"),
                    ("depthOfField", "tagVideoOption05"),
                    ("softParticles", "tagVideoOption06"),
                    ("hdrLighting", "tagVideoOption16"),
                    ("fog", "tagVideoOption32"),
                ],
                640.0,
                30.0,
            );
        }
        Tab::KeyBindings => {
            p.y = 383.0;
            p.check("gamepadSupport", "tagGameplayOption24S", 50.0, 40.0);
            p.y = 383.0;
            p.drop(Shown::Movement, "tagGameplayOption35", 470.0, 619.0, 200.0);
        }
        Tab::Network => {}
    }
    p.ctls
}

/// Labels drawn on their own (left of a value-column checkbox): (tag, x,
/// centre y).
pub(super) fn loose_labels(tab: Tab) -> &'static [(&'static str, f32, f32)] {
    match tab {
        Tab::Video => &[("tagVideoOption13", 557.0, 485.0)],
        _ => &[],
    }
}

/// The environment a dropdown's rows / shown text read besides the draft: the audio device lists, the render
/// adapter list, the mounted localisations, the effective primary monitor
/// ([`crate::settings::primary_monitor`], its video modes and physical size), and the localisation tag table
/// ([`gd_formats::text::TagTable`]) the value labels resolve through. `screen` is the window's physical
/// size, the resolution fallback when there is no monitor. Built once per calling system; the monitor's modes
/// are session-stable.
pub(super) struct ChoiceCtx<'a> {
    devices: &'a crate::audio::devices::AudioDevices,
    pub adapters: &'a crate::render_devices::RenderAdapters,
    pub languages: &'a [&'static gd_formats::text::Language],
    pub tags: &'a gd_formats::text::TagTable,
    monitor: Option<&'a bevy::window::Monitor>,
    screen: Vec2,
}

impl<'a> ChoiceCtx<'a> {
    pub(super) fn new(
        devices: &'a crate::audio::devices::AudioDevices,
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
    entries: &[crate::audio::devices::AudioDeviceEntry],
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
fn device_shown(index: i32, entries: &[crate::audio::devices::AudioDeviceEntry]) -> String {
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
