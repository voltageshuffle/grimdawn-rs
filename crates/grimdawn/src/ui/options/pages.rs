use super::*;

/// The page origin in the reference shots' coordinates: [`ORIGIN`] + `pageX` / `pageY` (15, 114).
pub(super) const PAGE: Vec2 = Vec2::new(ORIGIN.x + 15.0, ORIGIN.y + 114.0);

/// A page under construction (builders of sub_1400C8E60): `y` is the page's row
/// cursor (+552). A check box (sub_1400C4B80: x, dy) sits at (x + 20, y), then y += dy; a slider (sub_1400C4DD0)
/// is a 215 x 20 track at (x, y), its label 181 px left of it, then y += 35; a dropdown (sub_1400C5190 /
/// sub_1400C7010 / sub_1400C7780 / sub_1400C6A60: label x, box x, width; 200 when not given) is a width x 23 box at
/// (box x, y), its label at (label x, y), then y += 35. A new page's cursor is 20 (page ctor sub_1400C3AF0).
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
            // (`tagGameplayOption17`, int 4, only with more than one entry: its list gets one, so never shown.)
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
            // Anti-aliasing (the DirectX 11 deferred path: a check box, bool 52, label on its left: [`loose_labels`]).
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
            // "Fullscreen Gamma" (float 8; page builder sub_1400C8E60 → sub_1400C4DD0 idx 8): the exclusive
            // fullscreen gamma ramp (gd-render `post.rs` `with_gamma_ramp`).
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

pub(super) fn shown_text(s: Shown, o: &GameOptions, screen: Vec2) -> String {
    match s {
        // PORT: the port has English text only and one audio / render
        // device.
        Shown::Language => "English (US)".into(),
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
        Shown::OutputDevice | Shown::InputDevice => "Default".into(),
        Shown::RenderDevice => "[1] wgpu".into(),
        Shown::WindowMode => ["Windowed (Borderless)", "Windowed", "Fullscreen"]
            .get(o.int("screenMode").clamp(0, 2) as usize)
            .copied()
            .unwrap_or_default()
            .into(),
        Shown::Resolution => format!("{} x {}", screen.x as u32, screen.y as u32),
        Shown::Aniso => match o.int("anisotropicFiltering") {
            n if n <= 1 => "Off".into(),
            n => format!("{n}x"),
        },
        Shown::Quality(_) | Shown::Weather | Shown::Shadows => {
            let i = selected(s, o);
            // The list's entry (the engine shows the selected item's text).
            match s {
                Shown::Quality(_) => QUALITY[i].0,
                _ if i == 0 => "Off",
                _ => QUALITY[i - 1].0,
            }
            .into()
        }
        Shown::ColorBlind => ["Normal", "Protanopia", "Deuteranopia", "Tritanopia"]
            .get(o.int("colorblind").clamp(0, 3) as usize)
            .copied()
            .unwrap_or_default()
            .into(),
        Shown::Movement => ["Classic", "Keyboard", "Keyboard Only"]
            .get(o.int("movementType").clamp(0, 2) as usize)
            .copied()
            .unwrap_or_default()
            .into(),
    }
}

/// Quality names (`Options::PrintToConsole`, Engine.dll 0x180183240) and their file spelling.
pub(super) const QUALITY: [(&str, &str); 5] = [
    ("Low", "low"),
    ("Medium", "medium"),
    ("High", "high"),
    ("Very High", "very high"),
    ("Ultra", "ultra"),
];

/// Number of quality entries a dropdown offers (without the leading Off of weather / shadows). Jump table
/// sub_1400C5190 @0x1400C59B0, pick handler sub_1400C4720, page builder sub_1400C8E60: textureQuality
/// Low..High (3); fxQuality, lightingQuality and weather Low..Very High (4); detailLevel (5); shadows Low..Ultra (5)
/// after Off.
pub(super) fn level_count(s: Shown) -> usize {
    match s {
        Shown::Quality("textureQuality") => 3,
        Shown::Quality("detailLevel") | Shown::Shadows => 5,
        _ => 4,
    }
}

pub(super) fn levels<T>(n: usize, f: impl Fn((&'static str, &'static str)) -> T) -> Vec<T> {
    QUALITY[..n].iter().copied().map(f).collect()
}

/// The quality in the file as an index. `Options::Load` sub_1801821B0: "low" 0, "high" 2, "very high" 3,
/// "ultra" 4, anything else 1 (medium).
pub(super) fn quality_level(o: &GameOptions, k: &str) -> usize {
    match o.get(k) {
        Some(Value::Quality(q)) => QUALITY.iter().position(|(_, v)| v == q).unwrap_or(1),
        _ => 1,
    }
}

/// The list item a dropdown shows as selected. sub_1400C8E60: the stored quality index clamped to the entry count
/// (a file's "ultra" texture quality shows as High, the file value is kept until a pick); weather: item 0 when
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

/// A dropdown's rows: (text, the options a pick sets). PORT: the Language /
/// devices / resolution lists only hold what the port has; the others are
/// the engine's (see [`level_count`]; the file spells qualities in lower case).
pub(super) fn choices(
    s: Shown,
    o: &GameOptions,
    screen: Vec2,
) -> Vec<(String, Vec<(&'static str, Value)>)> {
    let ints = |k: &'static str, names: &[&str]| {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), vec![(k, Value::Int(i as i32))]))
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
        Shown::WindowMode => ints(
            "screenMode",
            &["Windowed (Borderless)", "Windowed", "Fullscreen"],
        ),
        Shown::ColorBlind => ints(
            "colorblind",
            &["Normal", "Protanopia", "Deuteranopia", "Tritanopia"],
        ),
        Shown::Movement => ints("movementType", &["Classic", "Keyboard", "Keyboard Only"]),
        Shown::Aniso => [1, 2, 4, 8, 16]
            .into_iter()
            .map(|n| {
                (
                    if n == 1 {
                        "Off".to_string()
                    } else {
                        format!("{n}x")
                    },
                    vec![("anisotropicFiltering", Value::Int(n))],
                )
            })
            .collect(),
        Shown::Quality(k) => levels(level_count(s), |(n, v)| {
            (n.to_string(), vec![(k, Value::Quality(v.to_string()))])
        }),
        Shown::Weather => std::iter::once((
            "Off".to_string(),
            vec![("weatherEnabled", Value::Bool(false))],
        ))
        .chain(levels(level_count(s), |(n, v)| {
            (
                n.to_string(),
                vec![
                    ("weatherEnabled", Value::Bool(true)),
                    ("weatherQuality", Value::Quality(v.to_string())),
                ],
            )
        }))
        .collect(),
        Shown::Shadows => {
            std::iter::once(("Off".to_string(), vec![("shadows", Value::Bool(false))]))
                .chain(levels(level_count(s), |(n, v)| {
                    (
                        n.to_string(),
                        vec![
                            ("shadows", Value::Bool(true)),
                            ("shadowQuality", Value::Quality(v.to_string())),
                        ],
                    )
                }))
                .collect()
        }
        Shown::Language
        | Shown::OutputDevice
        | Shown::InputDevice
        | Shown::RenderDevice
        | Shown::Resolution => vec![(shown_text(s, o, screen), Vec::new())],
    }
}

/// The knob's bitmap index into [`KNOB`] (the slider's draw sub_1400EC190): held while dragged, else lit
/// while the cursor is on the knob, else up.
pub(super) fn knob_bitmap(dragged: bool, hovered: bool) -> usize {
    match (dragged, hovered) {
        (true, _) => 1,
        (false, true) => 2,
        _ => 0,
    }
}

/// The rollover text tag of the control labelled `label` (the page builders sub_1400C8E60, sub_1400C7EB0,
/// sub_1400C59D0, sub_1400C6280: each check box / slider / dropdown gets its label's `tag<Page>Option<NN>` with
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
