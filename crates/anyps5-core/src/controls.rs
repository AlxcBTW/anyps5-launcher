//! Keyboard and mouse bindings, the tuning values and the text of the per-game `anyps5-input.ini`
//! (PORT.md 6.7, Controls.swift; file format: upstream `docs/user/INPUT_MAPPING.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A pad input a key or mouse button can press, named as the engine's input file names it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum PadAction {
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Options,
    TouchLeft,
    TouchRight,
    Up,
    Down,
    Left,
    Right,
    LeftStickUp,
    LeftStickDown,
    LeftStickLeft,
    LeftStickRight,
    RightStickUp,
    RightStickDown,
    RightStickLeft,
    RightStickRight,
    /// Held, the keyboard sticks tilt only by the tuning's stick modifier scale (walking).
    StickModifier,
}

impl PadAction {
    /// Every action, in the order of the ini lines.
    pub const ALL: [PadAction; 26] = [
        PadAction::Cross,
        PadAction::Circle,
        PadAction::Square,
        PadAction::Triangle,
        PadAction::L1,
        PadAction::R1,
        PadAction::L2,
        PadAction::R2,
        PadAction::L3,
        PadAction::R3,
        PadAction::Options,
        PadAction::TouchLeft,
        PadAction::TouchRight,
        PadAction::Up,
        PadAction::Down,
        PadAction::Left,
        PadAction::Right,
        PadAction::LeftStickUp,
        PadAction::LeftStickDown,
        PadAction::LeftStickLeft,
        PadAction::LeftStickRight,
        PadAction::RightStickUp,
        PadAction::RightStickDown,
        PadAction::RightStickLeft,
        PadAction::RightStickRight,
        PadAction::StickModifier,
    ];

    /// The name used in the ini file and as the key of the settings JSON.
    pub fn name(self) -> &'static str {
        match self {
            PadAction::Cross => "Cross",
            PadAction::Circle => "Circle",
            PadAction::Square => "Square",
            PadAction::Triangle => "Triangle",
            PadAction::L1 => "L1",
            PadAction::R1 => "R1",
            PadAction::L2 => "L2",
            PadAction::R2 => "R2",
            PadAction::L3 => "L3",
            PadAction::R3 => "R3",
            PadAction::Options => "Options",
            PadAction::TouchLeft => "TouchLeft",
            PadAction::TouchRight => "TouchRight",
            PadAction::Up => "Up",
            PadAction::Down => "Down",
            PadAction::Left => "Left",
            PadAction::Right => "Right",
            PadAction::LeftStickUp => "LeftStickUp",
            PadAction::LeftStickDown => "LeftStickDown",
            PadAction::LeftStickLeft => "LeftStickLeft",
            PadAction::LeftStickRight => "LeftStickRight",
            PadAction::RightStickUp => "RightStickUp",
            PadAction::RightStickDown => "RightStickDown",
            PadAction::RightStickLeft => "RightStickLeft",
            PadAction::RightStickRight => "RightStickRight",
            PadAction::StickModifier => "StickModifier",
        }
    }

    pub fn from_name(name: &str) -> Option<PadAction> {
        PadAction::ALL.into_iter().find(|a| a.name() == name)
    }
}

/// Settings for the controller sticks, the mouse as the right stick and the keyboard sticks, in
/// the steps the launcher shows: whole percentages, tenths and milliseconds. The defaults are the
/// engine's; only changed values are written. Missing keys in a saved file keep their default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InputTuning {
    pub left_deadzone: i32,
    pub right_deadzone: i32,
    pub mouse_sensitivity: i32,
    pub mouse_vertical_scale: i32,
    pub mouse_curve: i32,
    pub mouse_deadzone_offset: i32,
    pub mouse_max_speed: i32,
    pub mouse_smoothing_ms: i32,
    pub mouse_decay_ms: i32,
    pub stick_modifier_scale: i32,
    pub keyboard_ramp_ms: i32,
    pub trigger_ramp_ms: i32,
}

impl Default for InputTuning {
    fn default() -> Self {
        InputTuning {
            left_deadzone: 0,
            right_deadzone: 0,
            mouse_sensitivity: 10,
            mouse_vertical_scale: 10,
            mouse_curve: 10,
            mouse_deadzone_offset: 20,
            mouse_max_speed: 10,
            mouse_smoothing_ms: 10,
            mouse_decay_ms: 30,
            stick_modifier_scale: 50,
            keyboard_ramp_ms: 0,
            trigger_ramp_ms: 0,
        }
    }
}

impl InputTuning {
    /// `Setting = Value` lines for the values that differ from the defaults after clamping to the
    /// ranges the engine accepts, in the file's order.
    pub fn lines(&self) -> Vec<String> {
        let defaults = InputTuning::default();
        // (name, clamped value, default, divisor: 1 = integer)
        let settings: [(&str, i32, i32, i32); 12] = [
            (
                "LeftStickDeadzone",
                self.left_deadzone.clamp(0, 90),
                defaults.left_deadzone,
                1,
            ),
            (
                "RightStickDeadzone",
                self.right_deadzone.clamp(0, 90),
                defaults.right_deadzone,
                1,
            ),
            (
                "MouseSensitivity",
                self.mouse_sensitivity.clamp(1, 100),
                defaults.mouse_sensitivity,
                10,
            ),
            (
                "MouseVerticalScale",
                self.mouse_vertical_scale.clamp(1, 100),
                defaults.mouse_vertical_scale,
                10,
            ),
            (
                "MouseCurve",
                self.mouse_curve.clamp(2, 50),
                defaults.mouse_curve,
                10,
            ),
            (
                "MouseDeadzoneOffset",
                self.mouse_deadzone_offset.clamp(0, 90),
                defaults.mouse_deadzone_offset,
                100,
            ),
            (
                "MouseMaxSpeed",
                self.mouse_max_speed.clamp(1, 100),
                defaults.mouse_max_speed,
                10,
            ),
            (
                "MouseSmoothingMs",
                self.mouse_smoothing_ms.clamp(0, 500),
                defaults.mouse_smoothing_ms,
                1,
            ),
            (
                "MouseDecayMs",
                self.mouse_decay_ms.clamp(1, 1000),
                defaults.mouse_decay_ms,
                1,
            ),
            (
                "StickModifierScale",
                self.stick_modifier_scale.clamp(5, 100),
                defaults.stick_modifier_scale,
                100,
            ),
            (
                "KeyboardRampMs",
                self.keyboard_ramp_ms.clamp(0, 2000),
                defaults.keyboard_ramp_ms,
                1,
            ),
            (
                "TriggerRampMs",
                self.trigger_ramp_ms.clamp(0, 2000),
                defaults.trigger_ramp_ms,
                1,
            ),
        ];
        settings
            .into_iter()
            .filter(|(_, value, default, _)| value != default)
            .map(|(name, value, _, divisor)| match divisor {
                1 => format!("{name} = {value}"),
                // Debug prints the shortest round-trip form with ".0" for whole numbers, like Swift's Double.
                d => format!("{name} = {:?}", f64::from(value) / f64::from(d)),
            })
            .collect()
    }
}

/// The user's moves away from the default layout: action name to its sources.
pub type Changes = BTreeMap<String, Vec<String>>;

/// The keyboard and mouse layout a game starts with: the launcher's own layout with the keys the
/// user moved. Every action is written to the input file, so none of the engine's scattered
/// built-in keys stay behind.
pub struct Controls;

impl Controls {
    /// Stands for "no key": the engine refuses an empty source and no offered key is F24.
    pub const UNBOUND: &'static str = "KEY:F24";

    /// First line of every input file.
    pub const HEADER: &'static str = "# Written by the AnyPS5 launcher at each start; change the keys in the game's settings there.";

    /// The launcher's default sources of an action (`KEY:<SDL name>`, `MOUSE:<button>`, `WHEEL:<Up|Down>`).
    pub fn default_sources(action: PadAction) -> &'static [&'static str] {
        use PadAction::*;
        match action {
            Cross => &["KEY:K", "KEY:Space", "KEY:Return"],
            Circle => &["KEY:L"],
            Square => &["KEY:J", "MOUSE:Left"],
            Triangle => &["KEY:I"],
            L1 => &["KEY:Q"],
            R1 => &["KEY:E"],
            L2 => &["KEY:U", "MOUSE:X1"],
            R2 => &["KEY:O", "MOUSE:Right"],
            L3 => &["KEY:Left Shift"],
            R3 => &["KEY:Left Ctrl"],
            Options => &["KEY:Escape"],
            TouchLeft => &["KEY:Backspace"],
            TouchRight => &["KEY:Tab"],
            Up => &["KEY:Up", "WHEEL:Up"],
            Down => &["KEY:Down", "WHEEL:Down"],
            Left => &["KEY:Left"],
            Right => &["KEY:Right"],
            LeftStickUp => &["KEY:W"],
            LeftStickDown => &["KEY:S"],
            LeftStickLeft => &["KEY:A"],
            LeftStickRight => &["KEY:D"],
            RightStickUp => &["KEY:T"],
            RightStickDown => &["KEY:G"],
            RightStickLeft => &["KEY:F"],
            RightStickRight => &["KEY:H"],
            StickModifier => &[],
        }
    }

    /// The sources an action has: the user's change if there is one (even an empty list), else
    /// the default layout.
    pub fn sources(action: PadAction, changes: &Changes) -> Vec<String> {
        match changes.get(action.name()) {
            Some(own) => own.clone(),
            None => Controls::default_sources(action)
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }

    /// The changes after giving `key` (a `KEY:` source) to `action`: it replaces the action's keys,
    /// keeps its mouse and wheel sources, and is taken away from every other action that had it.
    /// Entries equal to the default layout are dropped.
    pub fn assign(key: &str, to: PadAction, changes: &Changes) -> Changes {
        let mut result = changes.clone();
        for other in PadAction::ALL.into_iter().filter(|a| *a != to) {
            let current = Controls::sources(other, changes);
            if current.iter().any(|s| s == key) {
                result.insert(
                    other.name().to_string(),
                    current.into_iter().filter(|s| s != key).collect(),
                );
            }
        }
        let mut own = vec![key.to_string()];
        own.extend(
            Controls::sources(to, changes)
                .into_iter()
                .filter(|s| !s.starts_with("KEY:")),
        );
        result.insert(to.name().to_string(), own);
        result.retain(|name, value| {
            PadAction::from_name(name)
                .is_some_and(|a| Controls::default_sources(a) != value.as_slice())
        });
        result
    }

    /// The text of `anyps5-input.ini` for the changes and the tuning. UTF-8, LF, final newline.
    pub fn file(changes: &Changes, tuning: &InputTuning) -> String {
        let mut lines = vec![Controls::HEADER.to_string()];
        for action in PadAction::ALL {
            let current = Controls::sources(action, changes);
            if current.is_empty() {
                lines.push(format!("{} = {}", action.name(), Controls::UNBOUND));
            }
            lines.extend(
                current
                    .into_iter()
                    .map(|source| format!("{} = {source}", action.name())),
            );
        }
        lines.extend(tuning.lines());
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// The SDL scancode name of a Windows set-1 scan code (`MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX)`:
/// extended keys as `0xE000 | code`), `None` for keys a binding cannot use. The key's position on a
/// US keyboard decides, as SDL reads scancodes. F24 is deliberately absent.
pub fn key_name(scancode: u16) -> Option<&'static str> {
    SCANCODES
        .iter()
        .find(|(code, _)| *code == scancode)
        .map(|(_, name)| *name)
}

/// The inverse of [`key_name`], for showing which physical key a binding means.
pub fn scancode_of(name: &str) -> Option<u16> {
    SCANCODES
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(code, _)| *code)
}

#[rustfmt::skip]
const SCANCODES: &[(u16, &str)] = &[
    (0x01, "Escape"), (0x02, "1"), (0x03, "2"), (0x04, "3"), (0x05, "4"), (0x06, "5"), (0x07, "6"), (0x08, "7"),
    (0x09, "8"), (0x0A, "9"), (0x0B, "0"), (0x0C, "-"), (0x0D, "="), (0x0E, "Backspace"), (0x0F, "Tab"),
    (0x10, "Q"), (0x11, "W"), (0x12, "E"), (0x13, "R"), (0x14, "T"), (0x15, "Y"), (0x16, "U"), (0x17, "I"),
    (0x18, "O"), (0x19, "P"), (0x1A, "["), (0x1B, "]"), (0x1C, "Return"), (0x1D, "Left Ctrl"),
    (0x1E, "A"), (0x1F, "S"), (0x20, "D"), (0x21, "F"), (0x22, "G"), (0x23, "H"), (0x24, "J"), (0x25, "K"),
    (0x26, "L"), (0x27, ";"), (0x28, "'"), (0x29, "`"), (0x2A, "Left Shift"), (0x2B, "\\"),
    (0x2C, "Z"), (0x2D, "X"), (0x2E, "C"), (0x2F, "V"), (0x30, "B"), (0x31, "N"), (0x32, "M"), (0x33, ","),
    (0x34, "."), (0x35, "/"), (0x36, "Right Shift"), (0x37, "Keypad *"), (0x38, "Left Alt"),
    (0x39, "Space"), (0x3A, "CapsLock"),
    (0x3B, "F1"), (0x3C, "F2"), (0x3D, "F3"), (0x3E, "F4"), (0x3F, "F5"), (0x40, "F6"), (0x41, "F7"),
    (0x42, "F8"), (0x43, "F9"), (0x44, "F10"), (0x45, "Numlock"), (0x46, "ScrollLock"),
    (0x47, "Keypad 7"), (0x48, "Keypad 8"), (0x49, "Keypad 9"), (0x4A, "Keypad -"), (0x4B, "Keypad 4"),
    (0x4C, "Keypad 5"), (0x4D, "Keypad 6"), (0x4E, "Keypad +"), (0x4F, "Keypad 1"), (0x50, "Keypad 2"),
    (0x51, "Keypad 3"), (0x52, "Keypad 0"), (0x53, "Keypad ."), (0x57, "F11"), (0x58, "F12"),
    (0xE01C, "Keypad Enter"), (0xE01D, "Right Ctrl"), (0xE035, "Keypad /"), (0xE037, "PrintScreen"),
    (0xE038, "Right Alt"), (0xE047, "Home"), (0xE048, "Up"), (0xE049, "PageUp"), (0xE04B, "Left"),
    (0xE04D, "Right"), (0xE04F, "End"), (0xE050, "Down"), (0xE051, "PageDown"), (0xE052, "Insert"),
    (0xE053, "Delete"), (0xE05B, "Left GUI"), (0xE05C, "Right GUI"), (0xE05D, "Application"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const GOLDEN: &str = "# Written by the AnyPS5 launcher at each start; change the keys in the game's settings there.
Cross = KEY:K
Cross = KEY:Space
Cross = KEY:Return
Circle = KEY:L
Square = KEY:J
Square = MOUSE:Left
Triangle = KEY:I
L1 = KEY:Q
R1 = KEY:E
L2 = KEY:U
L2 = MOUSE:X1
R2 = KEY:O
R2 = MOUSE:Right
L3 = KEY:Left Shift
R3 = KEY:Left Ctrl
Options = KEY:Escape
TouchLeft = KEY:Backspace
TouchRight = KEY:Tab
Up = KEY:Up
Up = WHEEL:Up
Down = KEY:Down
Down = WHEEL:Down
Left = KEY:Left
Right = KEY:Right
LeftStickUp = KEY:W
LeftStickDown = KEY:S
LeftStickLeft = KEY:A
LeftStickRight = KEY:D
RightStickUp = KEY:T
RightStickDown = KEY:G
RightStickLeft = KEY:F
RightStickRight = KEY:H
StickModifier = KEY:F24
";

    fn changes(entries: &[(&str, &[&str])]) -> Changes {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    #[test]
    fn default_file_is_the_golden_text() {
        assert_eq!(
            Controls::file(&Changes::new(), &InputTuning::default()),
            GOLDEN
        );
    }

    #[test]
    fn all_26_actions_in_order_and_names_roundtrip() {
        assert_eq!(PadAction::ALL.len(), 26);
        assert_eq!(PadAction::ALL[0].name(), "Cross");
        assert_eq!(PadAction::ALL[25].name(), "StickModifier");
        for a in PadAction::ALL {
            assert_eq!(PadAction::from_name(a.name()), Some(a));
        }
        assert_eq!(PadAction::from_name("cross"), None);
    }

    #[test]
    fn changed_key_and_emptied_action() {
        let c = changes(&[("Circle", &["KEY:C"]), ("Triangle", &[])]);
        let text = Controls::file(&c, &InputTuning::default());
        assert!(text.contains("\nCircle = KEY:C\n"));
        assert!(!text.contains("Circle = KEY:L"));
        assert!(text.contains("\nTriangle = KEY:F24\n"));
    }

    #[test]
    fn tuning_lines_are_appended() {
        let tuning = InputTuning {
            left_deadzone: 15,
            mouse_sensitivity: 25,
            ..Default::default()
        };
        let text = Controls::file(&Changes::new(), &tuning);
        assert!(text.ends_with(
            "StickModifier = KEY:F24\nLeftStickDeadzone = 15\nMouseSensitivity = 2.5\n"
        ));
    }

    #[test]
    fn defaults_give_no_tuning_lines() {
        assert!(InputTuning::default().lines().is_empty());
    }

    #[test]
    fn each_tuning_setting_alone() {
        let one = |f: fn(&mut InputTuning)| {
            let mut t = InputTuning::default();
            f(&mut t);
            t.lines()
        };
        assert_eq!(one(|t| t.left_deadzone = 30), ["LeftStickDeadzone = 30"]);
        assert_eq!(one(|t| t.right_deadzone = 90), ["RightStickDeadzone = 90"]);
        assert_eq!(one(|t| t.mouse_sensitivity = 1), ["MouseSensitivity = 0.1"]);
        assert_eq!(
            one(|t| t.mouse_vertical_scale = 20),
            ["MouseVerticalScale = 2.0"]
        );
        assert_eq!(one(|t| t.mouse_curve = 25), ["MouseCurve = 2.5"]);
        assert_eq!(
            one(|t| t.mouse_deadzone_offset = 5),
            ["MouseDeadzoneOffset = 0.05"]
        );
        assert_eq!(one(|t| t.mouse_max_speed = 90), ["MouseMaxSpeed = 9.0"]);
        assert_eq!(one(|t| t.mouse_smoothing_ms = 0), ["MouseSmoothingMs = 0"]);
        assert_eq!(one(|t| t.mouse_decay_ms = 500), ["MouseDecayMs = 500"]);
        assert_eq!(
            one(|t| t.stick_modifier_scale = 100),
            ["StickModifierScale = 1.0"]
        );
        assert_eq!(one(|t| t.keyboard_ramp_ms = 120), ["KeyboardRampMs = 120"]);
        assert_eq!(one(|t| t.trigger_ramp_ms = 2000), ["TriggerRampMs = 2000"]);
        assert_eq!(
            one(|t| t.mouse_deadzone_offset = 90),
            ["MouseDeadzoneOffset = 0.9"]
        );
    }

    #[test]
    fn clamp_edges() {
        let one = |f: fn(&mut InputTuning)| {
            let mut t = InputTuning::default();
            f(&mut t);
            t.lines()
        };
        assert_eq!(one(|t| t.left_deadzone = 91), ["LeftStickDeadzone = 90"]);
        assert_eq!(
            one(|t| t.left_deadzone = -5),
            Vec::<String>::new(),
            "clamps to 0 = default"
        );
        assert_eq!(one(|t| t.mouse_sensitivity = 0), ["MouseSensitivity = 0.1"]);
        assert_eq!(
            one(|t| t.mouse_sensitivity = 101),
            ["MouseSensitivity = 10.0"]
        );
        assert_eq!(one(|t| t.mouse_curve = 1), ["MouseCurve = 0.2"]);
        assert_eq!(one(|t| t.mouse_curve = 51), ["MouseCurve = 5.0"]);
        assert_eq!(
            one(|t| t.stick_modifier_scale = 4),
            ["StickModifierScale = 0.05"]
        );
        assert_eq!(one(|t| t.mouse_decay_ms = 0), ["MouseDecayMs = 1"]);
        assert_eq!(one(|t| t.mouse_decay_ms = 1001), ["MouseDecayMs = 1000"]);
        assert_eq!(
            one(|t| t.keyboard_ramp_ms = 2001),
            ["KeyboardRampMs = 2000"]
        );
        assert_eq!(
            one(|t| t.mouse_smoothing_ms = 501),
            ["MouseSmoothingMs = 500"]
        );
        assert_eq!(
            one(|t| t.mouse_deadzone_offset = 91),
            ["MouseDeadzoneOffset = 0.9"]
        );
        // a value that clamps onto the default writes nothing
        assert!(one(|t| t.mouse_curve = 10).is_empty());
    }

    #[test]
    fn line_order_follows_the_file_order() {
        let t = InputTuning {
            trigger_ramp_ms: 5,
            left_deadzone: 1,
            mouse_decay_ms: 40,
            ..Default::default()
        };
        assert_eq!(
            t.lines(),
            [
                "LeftStickDeadzone = 1",
                "MouseDecayMs = 40",
                "TriggerRampMs = 5"
            ]
        );
    }

    #[test]
    fn tuning_json_defaults_and_names() {
        let t: InputTuning = serde_json::from_str(r#"{"leftDeadzone": 12}"#).unwrap();
        assert_eq!(t.left_deadzone, 12);
        assert_eq!(t.mouse_decay_ms, 30);
        let json = serde_json::to_value(InputTuning::default()).unwrap();
        assert_eq!(json["mouseDeadzoneOffset"], 20);
        assert_eq!(json.as_object().unwrap().len(), 12);
    }

    #[test]
    fn sources_prefer_changes_even_when_empty() {
        let c = changes(&[("Cross", &[])]);
        assert!(Controls::sources(PadAction::Cross, &c).is_empty());
        assert_eq!(Controls::sources(PadAction::Circle, &c), ["KEY:L"]);
        assert!(Controls::sources(PadAction::StickModifier, &c).is_empty());
    }

    #[test]
    fn assigning_a_taken_key_moves_it() {
        let c = Controls::assign("KEY:W", PadAction::Cross, &Changes::new());
        assert_eq!(c["Cross"], ["KEY:W"]);
        assert_eq!(c["LeftStickUp"], Vec::<String>::new());
        assert_eq!(c.len(), 2);
        let text = Controls::file(&c, &InputTuning::default());
        assert!(text.contains("Cross = KEY:W\n") && text.contains("LeftStickUp = KEY:F24\n"));
        assert!(!text.contains("Cross = KEY:K"));
    }

    #[test]
    fn assign_keeps_mouse_and_wheel_sources() {
        let c = Controls::assign("KEY:Z", PadAction::Up, &Changes::new());
        assert_eq!(c["Up"], ["KEY:Z", "WHEEL:Up"]);
        let c = Controls::assign("KEY:Z", PadAction::Square, &Changes::new());
        assert_eq!(c["Square"], ["KEY:Z", "MOUSE:Left"]);
    }

    #[test]
    fn assigning_the_default_back_removes_the_entry_and_is_idempotent() {
        let c = Controls::assign("KEY:Z", PadAction::Circle, &Changes::new());
        assert_eq!(c.len(), 1);
        assert_eq!(Controls::assign("KEY:Z", PadAction::Circle, &c), c);
        let back = Controls::assign("KEY:L", PadAction::Circle, &c);
        assert!(back.is_empty());
    }

    #[test]
    fn assign_drops_unknown_action_names() {
        let c = changes(&[("Bogus", &["KEY:A"])]);
        assert!(!Controls::assign("KEY:Z", PadAction::Circle, &c).contains_key("Bogus"));
    }

    #[test]
    fn scancode_table_is_consistent() {
        let names: HashSet<_> = SCANCODES.iter().map(|(_, n)| *n).collect();
        let codes: HashSet<_> = SCANCODES.iter().map(|(c, _)| *c).collect();
        assert_eq!(names.len(), SCANCODES.len(), "names unique");
        assert_eq!(codes.len(), SCANCODES.len(), "codes unique");
        assert!(SCANCODES.len() > 100);
        assert!(!names.contains("F24"));
        assert_eq!(key_name(0x2A), Some("Left Shift"));
        assert_eq!(key_name(0x36), Some("Right Shift"));
        assert_eq!(key_name(0x1D), Some("Left Ctrl"));
        assert_eq!(key_name(0xE01D), Some("Right Ctrl"));
        assert_eq!(key_name(0x38), Some("Left Alt"));
        assert_eq!(key_name(0xE038), Some("Right Alt"));
        assert_eq!(key_name(0x56), None);
        assert_eq!(key_name(0x64), None);
        assert_eq!(scancode_of("Space"), Some(0x39));
    }

    #[test]
    fn every_default_key_can_be_captured_again() {
        for action in PadAction::ALL {
            for source in Controls::default_sources(action) {
                if let Some(key) = source.strip_prefix("KEY:") {
                    assert!(scancode_of(key).is_some(), "{key} has no scan code");
                }
            }
        }
    }
}
