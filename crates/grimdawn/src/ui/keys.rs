//! Key bindings: the engine's key actions (`keybindings.txt`, parsed and written by `gd_formats::options`) with
//! the defaults of sub_1401F54E0 ([`DEFAULT_KEYS`]), edited in the options window's Key Bindings page and
//! stored in the port's own `keybindings.txt` (`crate::settings`; never the game's file).
//!
//! Key codes are DirectInput scan codes (1 Escape … 88 F12, 107 / 118 right Ctrl / Alt), 145 to 152 the mouse
//! ([`mouse_codes`]) and 153+ the gamepad (PORT: not honoured). Each action the port reads has a constant
//! below; its consumer reads it through [`KeyBindings::just_pressed`] / [`KeyBindings::pressed`]. Not handled
//! (sub_140211AF0 / sub_1402110D0): 5 Chat, 6 Group, 58 Push to Talk (multiplayer); 41 / 42 only flip
//! HUD bytes nothing reads; 67 Skill Reclamation Mode has no handler.
use bevy::prelude::*;
use gd_formats::options::{KeyMap, parse_keybindings, write_keybindings};

/// Actions the port reads (sub_1401F6310's tags).
pub const CHARACTER: u32 = 1;
pub const SKILLS: u32 = 2;
pub const CODEX: u32 = 3;
pub const MAP: u32 = 4;
/// "Help" (default H): the journal on its Help tab (`quests::toggle`).
pub const HELP: u32 = 8;
/// "Center Map (Map Window)" (`tagKeybind30`, default Numpad 0; `localmap::center_map_key`).
pub const CENTER_MAP: u32 = 31;
/// "Factions Window" (`tagKeybind47`, default J; `factions.rs`).
pub const FACTIONS: u32 = 52;
/// "Achievements Window" (`tagKeybind48`, default V; `achievements.rs`).
pub const ACHIEVEMENTS: u32 = 53;
pub const GAME_MENU: u32 = 7;
pub const DROP_ITEM: u32 = 32;
pub const WEAPON_SWAP: u32 = 34;
pub const TARGET_PET: u32 = 38;
pub const PAUSE: u32 = 40;
/// "Stationary Attack" (`tagKeybind36`, default Shift): held, `crate::casting::ClickKeys`.
pub const STATIONARY_ATTACK: u32 = 39;
/// "Force Move" (`tagForceMove`, default F): held, `crate::casting::ClickKeys`.
pub const FORCE_MOVE: u32 = 50;
/// "Altar Window" (`tagAltarWindow`, no default key; `crate::ascendant`).
pub const ALTAR_WINDOW: u32 = 61;
/// "Toggle UI" (`tagToggleUI`, default `]`; `hud::toggle_ui`).
pub const TOGGLE_UI: u32 = 60;
/// Move (`tagMove`): held, walks to the cursor (`crate::casting::MoveKeyHeld`).
pub const MOVE: u32 = 51;
/// Hot slot `i` (0..10, keys 1 to 0) is action `HOT_SLOT_FIRST + i`.
pub const HOT_SLOT_FIRST: u32 = 9;
pub const DRINK_ENERGY: u32 = 29;
/// "Personal Riftgate" (`tagKeybind31`, default L): presses the HUD teleport button shown (`ui/riftmap.rs`).
pub const PERSONAL_RIFTGATE: u32 = 33;
/// "Quickbar Switch" (`tagKeybind40`, default Y): the other hotbar row (`hud::buttons`).
pub const QUICKBAR_SWITCH: u32 = 43;
/// "Show Alternate Hot Slot Set" (`tagToggleHotslotSet`, default T): another skill set's hotbar page (`hud::buttons`).
pub const SHOW_ALTERNATE_SET: u32 = 62;
pub const DRINK_HEALTH: u32 = 30;
pub const SHOW_ALL_ITEMS: u32 = 35;
pub const HIDE_COMMON_ITEMS: u32 = 37;
pub const PET_SELECT_FIRST: u32 = 44;
pub const PET_SELECT_ALL: u32 = 49;
pub const EVADE: u32 = 59;
pub const MOVE_FORWARD: u32 = 63;
pub const MOVE_BACKWARD: u32 = 64;
pub const MOVE_LEFT: u32 = 65;
pub const MOVE_RIGHT: u32 = 66;
pub const GROUND_TOOLTIP: u32 = 36;
pub const INTERACT: u32 = 54;
pub const PICKUP: u32 = 55;
pub const LOOT_FILTER: u32 = 56;
pub const HIDE_ALL_ITEMS: u32 = 57;

/// The default bindings, classic movement (sub_1401F54E0): (action,
/// primary, secondary key code).
pub const DEFAULT_KEYS: [(u32, u32, u32); 57] = [
    (1, 46, 23),
    (2, 31, 0),
    (3, 16, 0),
    (4, 50, 154),
    (5, 28, 0),
    (6, 37, 0),
    (7, 34, 0),
    (8, 35, 0),
    (9, 2, 165),
    (10, 3, 167),
    (11, 4, 168),
    (12, 5, 164),
    (13, 6, 155),
    (14, 7, 156),
    (15, 8, 0),
    (16, 9, 0),
    (17, 10, 0),
    (18, 11, 0),
    (21, 145, 0),
    (22, 146, 0),
    (23, 79, 0),
    (24, 81, 0),
    (25, 80, 0),
    (26, 147, 0),
    (27, 51, 0),
    (28, 52, 0),
    (29, 18, 162),
    (30, 19, 161),
    (31, 82, 0),
    (32, 32, 0),
    (33, 38, 153),
    (34, 17, 159),
    (35, 56, 118),
    (36, 45, 0),
    (37, 44, 0),
    (38, 29, 107),
    (39, 42, 163),
    (40, 25, 0),
    (41, 14, 0),
    (42, 43, 0),
    (43, 21, 160),
    (44, 60, 0),
    (45, 61, 0),
    (46, 62, 0),
    (47, 63, 0),
    (48, 64, 0),
    (49, 65, 0),
    (50, 33, 0),
    (51, 30, 0),
    (52, 36, 0),
    (53, 47, 0),
    (54, 22, 0),
    (56, 24, 0),
    (58, 15, 0),
    (59, 57, 166),
    (60, 27, 0),
    (62, 20, 0),
];

/// Action → [primary, secondary] key code (0 = none): the classic-movement map (`keys`) and the keyboard-movement map
/// (`alt`, used while `movementType` is 1 or 2: `mode`). `sub_1401F54E0`: one default table,
/// whose last entries depend on the movement type: the keyboard map has Move_Forward / Backward / Left / Right (63 to 66)
/// on W / S / A / D, the skills window (2) on N, action 32 on B, the weapon swap's primary key (34) and Move (51)
/// unbound. Two files hold them (`keybindings.txt`, `alternate_keybindings.txt`).
#[derive(Resource, Clone, PartialEq, Debug)]
pub struct KeyBindings {
    pub keys: KeyMap,
    pub alt: KeyMap,
    /// The `movementType` option: which map is in use (0 classic, else `alt`).
    pub mode: i32,
    /// This frame's mouse codes ([`mouse_codes`]).
    mouse: MouseCodes,
}

/// The mouse's key codes 145 to 152 this frame (Wheel Up, Wheel Down, Middle Mouse, Mouse 4 to 8, the order of the
/// key names, [`key_name`]): bit `code − 145` of `pressed`, `+ 8` just pressed, `+ 16` just released. Written
/// by [`mouse_codes`] through a shared cell so that a frame's input does not mark the bindings changed; equal and
/// cloned as part of the bindings regardless of the frame.
#[derive(Clone, Default, Debug)]
struct MouseCodes(std::sync::Arc<std::sync::atomic::AtomicU32>);

impl PartialEq for MouseCodes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl MouseCodes {
    /// `code`'s (pressed, just pressed, just released).
    fn state(&self, code: u32) -> (bool, bool, bool) {
        let Some(bit) = code.checked_sub(MOUSE_FIRST).filter(|b| *b < 8) else {
            return (false, false, false);
        };
        let v = self.0.load(std::sync::atomic::Ordering::Relaxed);
        (
            v >> bit & 1 != 0,
            v >> (bit + 8) & 1 != 0,
            v >> (bit + 16) & 1 != 0,
        )
    }
}

/// Wheel Up: the first mouse code.
const MOUSE_FIRST: u32 = 145;

/// The bits of [`MouseCodes`] for a frame: the held buttons and those pressed / released (`buttons`, in code order
/// from Middle Mouse), the wheel's notches up / down (each a press and release in the frame, held for it).
fn mouse_bits(held: u8, pressed: u8, released: u8, wheel_up: bool, wheel_down: bool) -> u32 {
    let wheel = wheel_up as u32 | (wheel_down as u32) << 1;
    let b = |m: u8| (m as u32) << 2;
    (b(held) | wheel) | (b(pressed) | wheel) << 8 | (b(released) | wheel) << 16
}

/// The mouse buttons of codes 147 to 152. PORT: Mouse 6 to 8 are taken as the platform's further buttons
/// (`MouseButton::Other(6..=8)`); the engine reads DirectInput's buttons 2 to 7.
const MOUSE_BUTTONS: [MouseButton; 6] = [
    MouseButton::Middle,
    MouseButton::Back,
    MouseButton::Forward,
    MouseButton::Other(6),
    MouseButton::Other(7),
    MouseButton::Other(8),
];

/// Resets every key's state except Escape's: the menu / options / edit-box input systems swallow the
/// keyboard while they hold it (PreUpdate), but Escape belongs to the one dispatcher
/// (`ui::esc_dispatcher`, Update), which closes the frontmost window or opens the game menu.
pub fn reset_but_escape(keys: &mut ButtonInput<KeyCode>) {
    let ks: Vec<KeyCode> = keys
        .get_pressed()
        .chain(keys.get_just_released())
        .copied()
        .filter(|k| *k != KeyCode::Escape)
        .collect();
    for k in ks {
        keys.reset(k);
    }
}

/// Records this frame's mouse codes for the bindings (PreUpdate, after the input and before any window consumes
/// a click). While the game menu, its confirm or Options is up (`GamePause::menu`) the buttons and the wheel
/// count as neither held nor pressed (the menus' `mouse.reset_all()` runs after this), so actions bound to the
/// middle button, Mouse 4-8 or the wheel do nothing under them; releases still pass so a held action stops.
pub fn mouse_codes(
    bindings: Res<KeyBindings>,
    buttons: Res<ButtonInput<MouseButton>>,
    pause: Res<gd_game::pause::GamePause>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
) {
    let mask = |f: &dyn Fn(MouseButton) -> bool| {
        MOUSE_BUTTONS
            .iter()
            .enumerate()
            .fold(0u8, |m, (i, b)| m | (f(*b) as u8) << i)
    };
    let (mut up, mut down) = (false, false);
    for w in wheel.read() {
        up |= w.y > 0.0;
        down |= w.y < 0.0;
    }
    let live = !pause.menu;
    let bits = mouse_bits(
        mask(&|b| live && buttons.pressed(b)),
        mask(&|b| live && buttons.just_pressed(b)),
        mask(&|b| buttons.just_released(b)),
        live && up,
        live && down,
    );
    bindings
        .mouse
        .0
        .store(bits, std::sync::atomic::Ordering::Relaxed);
}

impl Default for KeyBindings {
    fn default() -> Self {
        let keys: KeyMap = DEFAULT_KEYS.iter().map(|&(a, p, s)| (a, [p, s])).collect();
        let mut alt = keys.clone();
        for (a, v) in [
            (2, [49, 0]),
            (32, [48, 0]),
            (34, [0, 159]),
            (51, [0, 0]),
            (63, [17, 0]),
            (64, [31, 0]),
            (65, [30, 0]),
            (66, [32, 0]),
        ] {
            alt.insert(a, v);
        }
        Self {
            keys,
            alt,
            mode: 0,
            mouse: MouseCodes::default(),
        }
    }
}

impl KeyBindings {
    /// The map in use.
    fn map(&self) -> &KeyMap {
        if self.mode > 0 { &self.alt } else { &self.keys }
    }

    fn map_mut(&mut self) -> &mut KeyMap {
        if self.mode > 0 {
            &mut self.alt
        } else {
            &mut self.keys
        }
    }

    pub fn get(&self, action: u32) -> [u32; 2] {
        self.map().get(&action).copied().unwrap_or([0, 0])
    }

    /// Binds `code` to `action`'s slot (0 primary, 1 secondary); the code
    /// leaves any other binding (PORT: the engine's duplicate rule is not
    /// decompiled).
    pub fn bind(&mut self, action: u32, slot: usize, code: u32) {
        for v in self.map_mut().values_mut() {
            for c in v.iter_mut() {
                if *c == code {
                    *c = 0;
                }
            }
        }
        self.map_mut().entry(action).or_insert([0, 0])[slot.min(1)] = code;
    }

    /// True when a key or mouse code bound to `action` was pressed this frame.
    pub fn just_pressed(&self, action: u32, keys: &ButtonInput<KeyCode>) -> bool {
        self.any(action, |k| keys.just_pressed(k), |m| m.1)
    }

    /// True when a key or mouse code bound to `action` was released this frame.
    pub fn just_released(&self, action: u32, keys: &ButtonInput<KeyCode>) -> bool {
        self.any(action, |k| keys.just_released(k), |m| m.2)
    }

    /// True while a key or mouse code bound to `action` is held.
    pub fn pressed(&self, action: u32, keys: &ButtonInput<KeyCode>) -> bool {
        self.any(action, |k| keys.pressed(k), |m| m.0)
    }

    /// [`Self::just_pressed`] without the wheel's codes (for a reader that takes the wheel's notches itself, the
    /// camera).
    pub fn just_pressed_no_wheel(&self, action: u32, keys: &ButtonInput<KeyCode>) -> bool {
        let wheel = MOUSE_FIRST..MOUSE_FIRST + 2;
        self.get(action)
            .iter()
            .filter(|c| **c != 0 && !wheel.contains(*c))
            .any(|&c| match keycode(c) {
                Some(k) => keys.just_pressed(k),
                None => self.mouse.state(c).1,
            })
    }

    /// `action`'s bound codes: a keyboard code through `key`, a mouse code's (pressed, just pressed, just released)
    /// through `mouse`.
    fn any(
        &self,
        action: u32,
        key: impl Fn(KeyCode) -> bool,
        mouse: impl Fn((bool, bool, bool)) -> bool,
    ) -> bool {
        self.get(action)
            .iter()
            .filter(|c| **c != 0)
            .any(|&c| match keycode(c) {
                Some(k) => key(k),
                None => mouse(self.mouse.state(c)),
            })
    }

    /// `keybindings.txt` lines: "action: primary secondary" (the classic map).
    pub fn parse(text: &str) -> Self {
        let mut b = Self::default();
        parse_keybindings(text, &mut b.keys);
        b
    }

    /// `alternate_keybindings.txt` (the keyboard map).
    pub fn parse_alt(&mut self, text: &str) {
        parse_keybindings(text, &mut self.alt);
    }

    pub fn write(&self) -> String {
        write_keybindings(&self.keys)
    }

    pub fn write_alt(&self) -> String {
        write_keybindings(&self.alt)
    }
}

/// The port's bindings file, beside its `options.txt`.
pub fn path() -> std::path::PathBuf {
    crate::settings::path().with_file_name("keybindings.txt")
}

/// The keyboard map's file (`alternate_keybindings.txt` in the engine).
pub fn alt_path() -> std::path::PathBuf {
    path().with_file_name("alternate_keybindings.txt")
}

pub fn load(mut commands: Commands) {
    let mut b = std::fs::read_to_string(path())
        .map(|t| KeyBindings::parse(&t))
        .unwrap_or_default();
    if let Ok(t) = std::fs::read_to_string(alt_path()) {
        b.parse_alt(&t);
    }
    commands.insert_resource(b);
}

/// The map in use follows the `movementType` option.
pub fn sync_mode(options: Res<gd_game::options::GameOptions>, mut b: ResMut<KeyBindings>) {
    let mode = options.int("movementType");
    if b.mode != mode {
        b.mode = mode;
    }
}

/// Writes both maps to the port's files (never into the game's folders).
pub fn store(b: &KeyBindings) {
    gd_game::session::write_user_file("key bindings", &path(), b.write());
    gd_game::session::write_user_file("alternate key bindings", &alt_path(), b.write_alt());
}

const KEYS: &[(u32, KeyCode)] = &{
    use KeyCode::*;
    [
        (1, Escape),
        (2, Digit1),
        (3, Digit2),
        (4, Digit3),
        (5, Digit4),
        (6, Digit5),
        (7, Digit6),
        (8, Digit7),
        (9, Digit8),
        (10, Digit9),
        (11, Digit0),
        (12, Minus),
        (13, Equal),
        (14, Backspace),
        (15, Tab),
        (16, KeyQ),
        (17, KeyW),
        (18, KeyE),
        (19, KeyR),
        (20, KeyT),
        (21, KeyY),
        (22, KeyU),
        (23, KeyI),
        (24, KeyO),
        (25, KeyP),
        (26, BracketLeft),
        (27, BracketRight),
        (28, Enter),
        (29, ControlLeft),
        (30, KeyA),
        (31, KeyS),
        (32, KeyD),
        (33, KeyF),
        (34, KeyG),
        (35, KeyH),
        (36, KeyJ),
        (37, KeyK),
        (38, KeyL),
        (39, Semicolon),
        (40, Quote),
        (41, Backquote),
        (42, ShiftLeft),
        (43, Backslash),
        (44, KeyZ),
        (45, KeyX),
        (46, KeyC),
        (47, KeyV),
        (48, KeyB),
        (49, KeyN),
        (50, KeyM),
        (51, Comma),
        (52, Period),
        (53, Slash),
        (54, ShiftRight),
        (55, NumpadMultiply),
        (56, AltLeft),
        (57, Space),
        (58, CapsLock),
        (59, F1),
        (60, F2),
        (61, F3),
        (62, F4),
        (63, F5),
        (64, F6),
        (65, F7),
        (66, F8),
        (67, F9),
        (68, F10),
        (69, NumLock),
        (70, ScrollLock),
        (71, Numpad7),
        (72, Numpad8),
        (73, Numpad9),
        (74, NumpadSubtract),
        (75, Numpad4),
        (76, Numpad5),
        (77, Numpad6),
        (78, NumpadAdd),
        (79, Numpad1),
        (80, Numpad2),
        (81, Numpad3),
        (82, Numpad0),
        (83, NumpadDecimal),
        (87, F11),
        (88, F12),
        (107, ControlRight),
        (118, AltRight),
    ]
};

/// A keyboard key code's Bevy key (module doc).
pub fn keycode(code: u32) -> Option<KeyCode> {
    KEYS.iter().find(|(c, _)| *c == code).map(|(_, k)| *k)
}

/// A Bevy key's engine key code.
pub fn code_of(key: KeyCode) -> Option<u32> {
    KEYS.iter().find(|(_, k)| *k == key).map(|(c, _)| *c)
}

/// An action's function tag (`tagKeybindNN`, the options list's names; sub_1401F6310).
pub fn action_tag(a: u32) -> &'static str {
    const T: [&str; 68] = [
        "",
        "tagKeybind01",
        "tagKeybind02",
        "tagKeybind03",
        "tagKeybind04",
        "tagKeybind05",
        "tagKeybind06",
        "tagKeybind07",
        "tagKeybind32",
        "tagKeybind08",
        "tagKeybind09",
        "tagKeybind10",
        "tagKeybind11",
        "tagKeybind12",
        "tagKeybind13",
        "tagKeybind14",
        "tagKeybind15",
        "tagKeybind16",
        "tagKeybind17",
        "",
        "",
        "tagKeybind20",
        "tagKeybind21",
        "tagKeybind22",
        "tagKeybind23",
        "tagKeybind34",
        "tagKeybind46",
        "tagKeybind44",
        "tagKeybind45",
        "tagKeybind19",
        "tagKeybind18",
        "tagKeybind30",
        "tagKeybind26",
        "tagKeybind31",
        "tagKeybind33",
        "tagKeybind27",
        "tagKeybind28",
        "tagKeybind29",
        "tagKeybind35",
        "tagKeybind36",
        "tagKeybind37",
        "tagKeybind38",
        "tagKeybind39",
        "tagKeybind40",
        "tagSelectPet1",
        "tagSelectPet2",
        "tagSelectPet3",
        "tagSelectPet4",
        "tagSelectPet5",
        "tagSelectAllPets",
        "tagForceMove",
        "tagMove",
        "tagKeybind47",
        "tagKeybind48",
        "tagUse",
        "tagPickup",
        "tagKeybind49",
        "tagKeybind50",
        "tagPushToTalk",
        "tagEvade",
        "tagToggleUI",
        "tagAltarWindow",
        "tagToggleHotslotSet",
        "tagMoveForward",
        "tagMoveBackward",
        "tagMoveLeft",
        "tagMoveRight",
        "tagSkillReclamationMode",
    ];
    T.get(a as usize).copied().unwrap_or("")
}

/// A key code's name (the options list and the tooltips; DirectInput names).
pub fn key_name(code: u32) -> String {
    const GAME: [&str; 28] = [
        "Wheel Up",
        "Wheel Down",
        "Middle Mouse",
        "Mouse 4",
        "Mouse 5",
        "Mouse 6",
        "Mouse 7",
        "Mouse 8",
        "Gamepad Up",
        "Gamepad Down",
        "Gamepad Left",
        "Gamepad Right",
        "Gamepad Start",
        "Gamepad Back",
        "Gamepad LThumb",
        "Gamepad RThumb",
        "Gamepad LBumper",
        "Gamepad RBumper",
        "Gamepad LTrigger",
        "Gamepad RTrigger",
        "Gamepad A",
        "Gamepad B",
        "Gamepad X",
        "Gamepad Y",
        "Gamepad Paddle 1",
        "Gamepad Paddle 2",
        "Gamepad Paddle 3",
        "Gamepad Paddle 4",
    ];
    if (145..=172).contains(&code) {
        return GAME[(code - 145) as usize].into();
    }
    const DIK: [&str; 89] = [
        "",
        "Escape",
        "1",
        "2",
        "3",
        "4",
        "5",
        "6",
        "7",
        "8",
        "9",
        "0",
        "-",
        "=",
        "BackSpace",
        "Tab",
        "Q",
        "W",
        "E",
        "R",
        "T",
        "Y",
        "U",
        "I",
        "O",
        "P",
        "[",
        "]",
        "Return",
        "Control_L",
        "A",
        "S",
        "D",
        "F",
        "G",
        "H",
        "J",
        "K",
        "L",
        ";",
        "'",
        "`",
        "Shift_L",
        "\\",
        "Z",
        "X",
        "C",
        "V",
        "B",
        "N",
        "M",
        ",",
        ".",
        "/",
        "Shift_R",
        "KP_Multiply",
        "Alt_L",
        "space",
        "Caps_Lock",
        "F1",
        "F2",
        "F3",
        "F4",
        "F5",
        "F6",
        "F7",
        "F8",
        "F9",
        "F10",
        "Num_Lock",
        "Scroll_Lock",
        "KP_7",
        "KP_8",
        "KP_9",
        "KP_Subtract",
        "KP_4",
        "KP_5",
        "KP_6",
        "KP_Add",
        "KP_1",
        "KP_2",
        "KP_3",
        "KP_0",
        "KP_Decimal",
        "",
        "",
        "",
        "F11",
        "F12",
    ];
    match code {
        107 => "Control_R".into(),
        118 => "Alt_R".into(),
        c => DIK.get(c as usize).copied().unwrap_or("").into(),
    }
}
