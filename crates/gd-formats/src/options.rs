//! The game's settings files: `options.txt` ([`Options`]) and `keybindings.txt` /
//! `alternate_keybindings.txt` ([`parse_keybindings`]).
//!
//! Engine.dll `Options::Options` @0x1801808d0: the option table
//! (name → index in the bool / int / float / quality / vector / string
//! arrays, [`table`]) and the defaults: the bool array is a 77-bit
//! `vector<bool>` resized with `true` (sub_1801837C0 → `resize(77, true)`),
//! then the constructor clears or sets single bits, so a bool it does not
//! touch defaults to on; ints and floats default to 0 unless set (master /
//! music / effects / ambient / dialog / voice volume 1, corpse persistence
//! 0, auto loot radius 0.5, network MTU 1400, anti-aliasing 1,
//! anisotropic filtering 1, speaker type 1, max resource threads 1).
//! PORT: the defaults of the floats and qualities the constructor leaves to
//! the array set-up (UI scale 0.5, map zoom, qualities) are the values of a real `options.txt`.
//!
//! `options.txt` lines are `name = value`, names padded to 26 columns, bools `true` / `false`, vectors as two
//! numbers, strings quoted. [`table`] holds every option the engine's constructor sets (including some the port
//! does not act on, e.g. `networkAdapter`, `UPnPEnable`); every other line of a read file (the game's
//! `antiAliasing`, `tripleBuffer`, `alphatocoverage`, `deferredrendering`, `refreshRate`, `windowPosition`,
//! `device`, ...) is kept verbatim and written back, so a shared `options.txt` loses nothing.
//!
//! The key binding files map an action to two key codes: lines `action: primary secondary` (0 = none).
use std::collections::BTreeMap;

/// An option's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Float(f32),
    /// `textureQuality = high` etc.
    Quality(String),
    Vec2(i32, i32),
    Str(String),
}

impl Value {
    fn parse_like(&self, s: &str) -> Option<Value> {
        let s = s.trim();
        Some(match self {
            Value::Bool(_) => Value::Bool(match s {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return None,
            }),
            Value::Int(_) => Value::Int(s.split_whitespace().next()?.parse().ok()?),
            Value::Float(_) => Value::Float(s.parse().ok()?),
            Value::Quality(_) => Value::Quality(s.to_owned()),
            Value::Vec2(..) => {
                let mut it = s.split_whitespace().map(|v| v.parse::<i32>().ok());
                Value::Vec2(it.next()??, it.next().flatten().unwrap_or(0))
            }
            Value::Str(_) => Value::Str(s.trim_matches('"').to_owned()),
        })
    }

    fn write(&self) -> String {
        match self {
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => format!("{f}"),
            Value::Quality(q) => q.clone(),
            Value::Vec2(a, b) => format!("{a} {b}"),
            Value::Str(s) => format!("\"{s}\""),
        }
    }
}

use Value::{Bool as B, Float as F, Int as I};

fn q(s: &str) -> Value {
    Value::Quality(s.to_owned())
}

/// The engine's options in `Options::Options` order: (name, default).
/// Bool / int / float indices (`Options::GetBool(n)` in the exe) in the
/// comments where code cites them.
pub fn table() -> &'static [(&'static str, Value)] {
    static TABLE: std::sync::LazyLock<Vec<(&'static str, Value)>> =
        std::sync::LazyLock::new(build_table);
    &TABLE
}

fn build_table() -> Vec<(&'static str, Value)> {
    vec![
        ("quickBuy", B(true)),                   // bool 0
        ("lockItemPickup", B(true)),             // bool 17
        ("mapZoom", F(65.0)),                    // float 5
        ("importedCharacters", B(false)),        // bool 18
        ("lockMapRotation", B(false)),           // bool 23
        ("cameraShake", B(true)),                // bool 8
        ("gore", B(true)),                       // bool 24
        ("questwidgetopen", B(true)),            // bool 25
        ("corpsePersistence", F(0.0)),           // float 9
        ("blood", I(0)),                         // int 4
        ("reportStats", I(0)),                   // int 6
        ("targetLock", B(false)),                // bool 27
        ("inactiveUpdateRate", F(0.0)),          // float 10
        ("cloudSaving", B(true)),                // bool 29
        ("gamepadSupport", B(true)),             // bool 32
        ("breakingMoveTo", B(true)),             // bool 33
        ("classicCasting", B(false)),            // bool 34
        ("gamepadTargetLock", B(true)),          // bool 38
        ("autoLootRadius", F(0.5)),              // float 13
        ("critFeedback", B(true)),               // bool 53
        ("dayNightCycle", B(true)),              // bool 60
        ("toggledAurasSelf", B(true)),           // bool 62
        ("toggledAurasOther", B(true)),          // bool 63
        ("toggledWeaponEffects", B(true)),       // bool 64
        ("detailItemTooltips", B(false)),        // bool 68
        ("evadeFollowCursor", B(true)),          // bool 75
        ("showMonsterLevelOnRollover", B(true)), // bool 5
        ("displayDamage", B(true)),              // bool 2
        ("critMultipliers", B(false)),           // bool 3
        ("targetOutline", B(true)),              // bool 22
        ("tutorialTips", B(true)),               // bool 26
        ("errorMessages", B(true)),              // bool 37
        ("autoItemTooltips", B(true)),           // bool 1
        ("extraRollovers", B(true)),             // bool 4
        ("sortConfirmBypass", B(false)),         // bool 47
        ("hotbarCooldownCounter", B(false)),     // bool 59
        ("showTime", B(false)),                  // bool 65
        ("mainMenu", I(0)),                      // int 12
        ("screenEffects", B(true)),              // bool 66
        ("newItemHighlight", B(true)),           // bool 67
        ("standardHUD", B(false)),               // bool 71
        ("transferConfirmBypass", B(true)),      // bool 73
        ("movementType", I(0)),                  // int 13
        ("largeCursor", B(false)),               // bool 76
        ("monsterBars", B(true)),                // bool 41
        ("playerBars", B(true)),                 // bool 39
        ("playerLocalBar", B(false)),            // bool 45
        ("petBars", B(false)),                   // bool 46
        ("playerHealth", B(true)),               // bool 40
        ("playerBarsLarge", B(false)),           // bool 54
        ("monsterHealth", B(true)),              // bool 42
        ("monsterIcons", B(true)),               // bool 43
        ("monsterBarsUndamaged", B(false)),      // bool 44
        ("monsterBarsLarge", B(false)),          // bool 55
        ("bossBarsLarge", B(false)),             // bool 56
        ("healthBarsPercent", B(false)),         // bool 57
        ("monsterBarsDebuffs", B(false)),        // bool 58
        ("textureQuality", q("high")),
        ("shadowQuality", q("medium")),
        ("reflectionQuality", q("high")),
        ("shadows", B(true)), // bool 16
        ("weatherQuality", q("low")),
        ("weatherEnabled", B(true)), // bool 21
        ("depthOfField", B(true)),   // bool 19
        ("softParticles", B(true)),  // bool 20
        ("hdrLighting", B(false)),   // bool 70
        ("detailLevel", q("high")),
        ("resolution", Value::Vec2(1920, 1080)),
        ("anisotropicFiltering", I(1)), // int 2
        ("screenMode", I(0)),           // int 5
        ("syncToRefresh", B(true)),     // bool 6
        ("detailObjects", B(true)),     // bool 9
        ("advancedEffects", B(true)),   // bool 10
        ("fxQuality", q("medium")),
        ("lightingQuality", q("medium")),
        ("uiScale", F(0.5)),    // float 11
        ("brightness", F(0.5)), // float 6 (the fullscreen gamma ramp, gd-render `post.rs`)
        ("contrast", F(0.5)),   // float 7
        ("gamma", F(0.5)),      // float 8, "Fullscreen Gamma"
        ("colorblind", I(0)),
        ("ambientocclusion", B(true)), // bool 30
        ("fxaa", B(true)),             // bool 52
        ("fog", B(true)),              // bool 61
        ("masterVolume", F(1.0)),      // float 0
        ("musicVolume", F(1.0)),       // float 1
        ("effectsVolume", F(1.0)),     // float 2
        ("ambientVolume", F(1.0)),     // float 3
        ("dialogVolume", F(1.0)),      // float 4
        ("rockOn", B(true)),           // bool 13
        ("speakerType", I(1)),         // int 9
        ("soundDevice", I(0)),
        ("errorSpeech", B(true)), // bool 36
        ("captureDevice", I(0)),
        ("micLoopback", B(false)),         // bool 48
        ("voiceVolume", F(1.0)),           // float 12
        ("voicechatEnabled", B(false)),    // bool 49
        ("voicechatPushToTalk", B(false)), // bool 50
        ("networkAdapter", Value::Str(String::new())),
        ("networkMTU", I(1400)),
        ("UPnPEnable", B(true)), // bool 28
        ("language", Value::Str("English".into())),
        ("datapath", Value::Str(String::new())),
        ("developer", B(false)),                // bool 12
        ("forceSingleProcessorMode", B(false)), // bool 11
        ("maxResourceThreads", I(1)),
        ("skipCompatibilityChecks", B(false)), // bool 14
        ("useArchives", B(true)),              // bool 15
        ("setupComplete", B(false)),           // bool 35
        ("v1200FirstRun", B(false)),           // bool 69
        ("v1300FirstRun", B(false)),           // bool 72
        ("gdx3FirstRun", B(false)),            // bool 74
    ]
}

/// An `options.txt`: the [`table`] options' values and the lines the port does not know.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    values: BTreeMap<&'static str, Value>,
    /// Lines of a read file whose name is not in [`table`] (e.g. `tripleBuffer`, `refreshRate`), kept verbatim
    /// and written back after the known ones.
    unknown: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            values: table().iter().cloned().collect(),
            unknown: Vec::new(),
        }
    }
}

impl Options {
    /// Defaults overridden by the `name = value` lines of `text` (unparsable values are skipped; lines with a name
    /// the port does not know are kept verbatim for [`Self::write`]).
    pub fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            if let Some(old) = o.values.get_mut(k) {
                if let Some(nv) = old.parse_like(v) {
                    *old = nv;
                }
            } else if !k.is_empty() {
                o.unknown.push(line.trim_end().to_owned());
            }
        }
        o
    }

    /// The file text, in the engine's order.
    pub fn write(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        for (name, _) in table() {
            if let Some(v) = self.values.get(name) {
                let _ = writeln!(s, "{name:<26}= {}", v.write());
            }
        }
        // PORT: the real game's options this port neither uses nor shows stay in the file as they were read.
        for line in &self.unknown {
            s.push_str(line);
            s.push('\n');
        }
        s
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    /// A bool option (false for an unknown name).
    pub fn flag(&self, name: &str) -> bool {
        debug_assert!(self.values.contains_key(name), "unknown option {name}");
        matches!(self.values.get(name), Some(Value::Bool(true)))
    }

    /// A float option (0 for an unknown name).
    pub fn float(&self, name: &str) -> f32 {
        debug_assert!(self.values.contains_key(name), "unknown option {name}");
        match self.values.get(name) {
            Some(Value::Float(f)) => *f,
            Some(Value::Int(i)) => *i as f32,
            _ => 0.0,
        }
    }

    pub fn int(&self, name: &str) -> i32 {
        match self.values.get(name) {
            Some(Value::Int(i)) => *i,
            Some(Value::Float(f)) => *f as i32,
            _ => 0,
        }
    }

    /// Sets a known option (same kind as its default); false otherwise.
    pub fn set(&mut self, name: &str, v: Value) -> bool {
        let Some(old) = self.values.get_mut(name) else {
            return false;
        };
        if std::mem::discriminant(old) != std::mem::discriminant(&v) {
            return false;
        }
        *old = v;
        true
    }
}

/// One key binding map: action → [primary, secondary] key code (0 = none).
pub type KeyMap = BTreeMap<u32, [u32; 2]>;

/// Reads `action: primary secondary` lines into `map` (over its defaults; a missing or unparsable code is 0, a
/// line without an action number is skipped).
pub fn parse_keybindings(text: &str, map: &mut KeyMap) {
    for line in text.lines() {
        let Some((a, rest)) = line.split_once(':') else {
            continue;
        };
        let Ok(a) = a.trim().parse::<u32>() else {
            continue;
        };
        let mut it = rest
            .split_whitespace()
            .map(|v| v.parse::<u32>().unwrap_or(0));
        map.insert(a, [it.next().unwrap_or(0), it.next().unwrap_or(0)]);
    }
}

/// The key binding file text, in action order.
pub fn write_keybindings(map: &KeyMap) -> String {
    map.iter()
        .map(|(a, [p, s])| format!("{a}: {p} {s}\n"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_lines_survive_a_round_trip() {
        let text = "gamma                     = 0.37\nmasterVolume              = 0.5\ntripleBuffer              = true\ndeferredrendering         = false\nresolution                = 1280 720\nrefreshRate               = 144 1\nbrightness                = 0.61\nantiAliasing              = 4\nfutureOption              = \"x y\"\n";
        let mut o = Options::parse(text);
        assert!(o.set("masterVolume", Value::Float(0.25)));
        let out = o.write();
        for l in text
            .lines()
            .filter(|l| !l.starts_with("masterVolume") && !l.starts_with("resolution"))
        {
            assert!(out.lines().any(|x| x == l), "lost {l}");
        }
        assert!(out.contains("masterVolume              = 0.25\n"));
        let back = Options::parse(&out);
        assert!(back == o && back.write() == out);
        assert!(!Options::default().write().contains("tripleBuffer"));
    }

    #[test]
    fn keybindings_round_trip() {
        let mut map = KeyMap::from([(1, [46, 0]), (2, [31, 0])]);
        parse_keybindings("2: 49 7\n9:2\nnot a line\nx: 4 5\n63: 17 junk\n", &mut map);
        assert_eq!(
            map,
            KeyMap::from([(1, [46, 0]), (2, [49, 7]), (9, [2, 0]), (63, [17, 0])])
        );
        let text = write_keybindings(&map);
        assert_eq!(text, "1: 46 0\n2: 49 7\n9: 2 0\n63: 17 0\n");
        let mut back = KeyMap::new();
        parse_keybindings(&text, &mut back);
        assert_eq!(back, map);
    }
}
