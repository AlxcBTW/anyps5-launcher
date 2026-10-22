//! The controls editor (docs/DESIGN.md 5.9, "Controls panel"): one row per pad action with its
//! key and mouse sources, key capture, reset. Bindings are the core's [`Controls`] changes inside
//! the game's [`GameSettings`]; the panel writes them and reports `changed` so the host saves.
//!
//! Capture: choosing a row sets `shell.input.capture_keys`; the next key press (physical position,
//! like SDL scancodes) replaces the row's keyboard keys, keeps its mouse sources and is taken away
//! from every other action. Esc cancels. Modifier keys count when pressed.

use anyps5_core::controls::{Controls, PadAction};
use anyps5_core::settings::GameSettings;
use egui::{Color32, Context, Key as EKey};

use crate::screens::rows::{self, Change, Row};
use crate::screens::side::{self, Output, SideState, SideView};
use crate::ui::Shell;
use crate::ui::components::PadGlyph;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;
use crate::ui::sound::Cue;

/// What the host has to do after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlsAction {
    /// Back or options: the panel is gone; open the game settings on the `controls` row.
    Back,
    /// The scrim was clicked: everything closes.
    Close,
}

/// The game, borrowed for one frame.
pub struct ControlsData<'a> {
    pub title: &'a str,
    pub settings: &'a mut GameSettings,
    pub language: Language,
    pub accent: Color32,
}

/// State of the controls panel.
#[derive(Default)]
pub struct ControlsPanel {
    side: SideState,
    capturing: Option<PadAction>,
}

impl ControlsPanel {
    /// Opens on the first key row (the hint above it is not focusable).
    pub fn open(&mut self) {
        self.side.open_at(1);
        self.capturing = None;
    }

    pub fn close(&mut self, shell: &mut Shell) {
        self.end_capture(shell);
        self.side.close();
    }

    pub fn dismiss(&mut self, shell: &mut Shell) {
        self.end_capture(shell);
        self.side.dismiss();
    }

    pub fn is_active(&self) -> bool {
        self.side.is_active()
    }

    /// The action waiting for a key, if any.
    pub fn capturing(&self) -> Option<PadAction> {
        self.capturing
    }

    /// Starts waiting for a key for `action` (the `controls-capture` state of the preview).
    pub fn begin_capture(&mut self, shell: &mut Shell, action: PadAction) {
        self.capturing = Some(action);
        shell.input.capture_keys = true;
    }

    fn end_capture(&mut self, shell: &mut Shell) {
        self.capturing = None;
        shell.input.capture_keys = false;
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &mut ControlsData<'_>,
    ) -> Output<ControlsAction> {
        let mut out = Output::default();
        if !self.side.is_active() {
            return out;
        }
        if self.side.is_open() {
            self.handle_events(shell, events, data, &mut out);
        }
        let mut rows = build_rows(data.settings, data.language, self.capturing);
        let view = SideView {
            title: data.title,
            subtitle: Some(data.language.text(Key::Controls)),
            list_id: "controls-rows",
            nav: &self.side.nav,
            accent: data.accent,
            language: data.language,
            force: None,
        };
        match side::draw(ctx, self.side.is_open(), &view, &mut rows) {
            Some(drawn) if self.side.is_open() => {
                let (changes, _) =
                    rows::apply_mouse(shell, &mut self.side.nav, &rows, drawn.events);
                for change in &changes {
                    self.apply(shell, data, change, &mut out);
                }
                if drawn.scrim_clicked {
                    shell.play(Cue::Back);
                    self.close(shell);
                    out.actions.push(ControlsAction::Close);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        out
    }

    fn handle_events(
        &mut self,
        shell: &mut Shell,
        events: &[InputEvent],
        data: &mut ControlsData<'_>,
        out: &mut Output<ControlsAction>,
    ) {
        for event in events {
            match (event, self.capturing) {
                // Waiting for a key: the key press binds, Esc and a pad's back cancel.
                (InputEvent::CapturedKey { key, physical_key, .. }, Some(action)) => {
                    if *key == EKey::Escape {
                        shell.play(Cue::Back);
                        self.end_capture(shell);
                    } else if let Some(name) = key_name(physical_key.unwrap_or(*key)) {
                        let current = data.settings.controls.clone().unwrap_or_default();
                        let next = Controls::assign(&format!("KEY:{name}"), action, &current);
                        data.settings.controls = (!next.is_empty()).then_some(next);
                        out.changed = true;
                        shell.play(Cue::Move);
                        self.end_capture(shell);
                    }
                    // A key that cannot be bound is ignored; the capture keeps waiting.
                }
                (InputEvent::Command(crate::ui::input::Command::Back), Some(_)) => {
                    shell.play(Cue::Back);
                    self.end_capture(shell);
                }
                (InputEvent::Command(_), Some(_)) => {}
                (InputEvent::Command(command), None) => {
                    let rows = build_rows(data.settings, data.language, None);
                    let driven = rows::drive(shell, &mut self.side.nav, &rows, &[*command]);
                    for change in &driven.changes {
                        self.apply(shell, data, change, out);
                    }
                    if driven.close {
                        // Back and options return to the game settings: no slide.
                        self.dismiss(shell);
                        out.actions.push(ControlsAction::Back);
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn apply(
        &mut self,
        shell: &mut Shell,
        data: &mut ControlsData<'_>,
        change: &Change,
        out: &mut Output<ControlsAction>,
    ) {
        match change {
            Change::Capture { row } => {
                if let Some(action) = PadAction::from_name(row) {
                    self.begin_capture(shell, action);
                }
            }
            Change::Run { row: "reset" } => {
                if data.settings.controls.take().is_some() {
                    out.changed = true;
                }
            }
            _ => {}
        }
    }
}

/// The rows: the hint, one per pad action, reset.
pub fn build_rows(
    settings: &GameSettings,
    language: Language,
    capturing: Option<PadAction>,
) -> Vec<Row> {
    let changes = settings.controls.clone().unwrap_or_default();
    let mut rows = vec![Row::info(
        "hint",
        language.text(Key::ControlsHint),
        None,
    )];
    for action in PadAction::ALL {
        let mut row = Row::keys(
            action.name(),
            action_title(action, language),
            Controls::sources(action, &changes),
            capturing == Some(action),
        )
        .icon(action_icon(action));
        if let Some(glyph) = face_glyph(action) {
            row = row.pad_glyph(glyph);
        }
        rows.push(row);
    }
    rows.push(
        Row::action("reset", language.text(Key::ResetControls)).icon(Icon::ARROW_COUNTER_CLOCKWISE),
    );
    rows
}

/// The controller face glyph in front of Cross, Circle, Square and Triangle.
fn face_glyph(action: PadAction) -> Option<PadGlyph> {
    match action {
        PadAction::Cross => Some(PadGlyph::Cross),
        PadAction::Circle => Some(PadGlyph::Circle),
        PadAction::Square => Some(PadGlyph::Square),
        PadAction::Triangle => Some(PadGlyph::Triangle),
        _ => None,
    }
}

/// The row title of an action.
pub fn action_title(action: PadAction, language: Language) -> String {
    let t = |key: Key| language.text(key);
    let dir = |stick: &str, key: Key| format!("{stick} · {}", t(key));
    let dpad = t(Key::Dpad);
    let left = t(Key::LeftStick);
    let right = t(Key::RightStick);
    match action {
        PadAction::Cross => "Cross".into(),
        PadAction::Circle => "Circle".into(),
        PadAction::Square => "Square".into(),
        PadAction::Triangle => "Triangle".into(),
        PadAction::L1 => "L1".into(),
        PadAction::R1 => "R1".into(),
        PadAction::L2 => "L2".into(),
        PadAction::R2 => "R2".into(),
        PadAction::L3 => "L3".into(),
        PadAction::R3 => "R3".into(),
        PadAction::Options => "Options".into(),
        PadAction::TouchLeft => t(Key::TouchpadLeft).into(),
        PadAction::TouchRight => t(Key::TouchpadRight).into(),
        PadAction::Up => dir(dpad, Key::DirectionUp),
        PadAction::Down => dir(dpad, Key::DirectionDown),
        PadAction::Left => dir(dpad, Key::DirectionLeft),
        PadAction::Right => dir(dpad, Key::DirectionRight),
        PadAction::LeftStickUp => dir(left, Key::DirectionUp),
        PadAction::LeftStickDown => dir(left, Key::DirectionDown),
        PadAction::LeftStickLeft => dir(left, Key::DirectionLeft),
        PadAction::LeftStickRight => dir(left, Key::DirectionRight),
        PadAction::RightStickUp => dir(right, Key::DirectionUp),
        PadAction::RightStickDown => dir(right, Key::DirectionDown),
        PadAction::RightStickLeft => dir(right, Key::DirectionLeft),
        PadAction::RightStickRight => dir(right, Key::DirectionRight),
        PadAction::StickModifier => t(Key::StickModifier).into(),
    }
}

/// The glyph of an action row.
pub fn action_icon(action: PadAction) -> Icon {
    match action {
        PadAction::Cross | PadAction::Circle | PadAction::Square | PadAction::Triangle => {
            Icon::GAME_CONTROLLER
        }
        PadAction::L1 | PadAction::R1 | PadAction::L2 | PadAction::R2 => Icon::RECTANGLE,
        PadAction::L3 | PadAction::R3 => Icon::CIRCLE,
        PadAction::Options => Icon::LIST,
        PadAction::TouchLeft | PadAction::TouchRight => Icon::HAND_POINTING,
        PadAction::Up | PadAction::Down | PadAction::Left | PadAction::Right => {
            Icon::ARROWS_OUT_CARDINAL
        }
        PadAction::LeftStickUp
        | PadAction::LeftStickDown
        | PadAction::LeftStickLeft
        | PadAction::LeftStickRight
        | PadAction::RightStickUp
        | PadAction::RightStickDown
        | PadAction::RightStickLeft
        | PadAction::RightStickRight => Icon::GAME_CONTROLLER,
        PadAction::StickModifier => Icon::PERSON_SIMPLE_WALK,
    }
}

/// The SDL scancode name of the physical key, as the input file names it. `None` for keys a
/// binding cannot use (the capture ignores them). Keypad keys arrive as their top-row twins.
pub fn key_name(key: EKey) -> Option<&'static str> {
    use EKey::*;
    Some(match key {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Num0 => "0",
        Num1 => "1",
        Num2 => "2",
        Num3 => "3",
        Num4 => "4",
        Num5 => "5",
        Num6 => "6",
        Num7 => "7",
        Num8 => "8",
        Num9 => "9",
        Minus => "-",
        Equals | Plus => "=",
        OpenBracket => "[",
        CloseBracket => "]",
        Backslash | IntlBackslash => "\\",
        Semicolon | Colon => ";",
        Quote => "'",
        Backtick => "`",
        Comma => ",",
        Period => ".",
        Slash => "/",
        Enter => "Return",
        Space => "Space",
        Tab => "Tab",
        Backspace => "Backspace",
        Escape => "Escape",
        Insert => "Insert",
        Delete => "Delete",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        ArrowLeft => "Left",
        ArrowRight => "Right",
        ArrowUp => "Up",
        ArrowDown => "Down",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        ShiftLeft => "Left Shift",
        ShiftRight => "Right Shift",
        ControlLeft => "Left Ctrl",
        ControlRight => "Right Ctrl",
        AltLeft => "Left Alt",
        AltRight => "Right Alt",
        SuperLeft => "Left GUI",
        SuperRight => "Right GUI",
        CapsLock => "CapsLock",
        NumLock => "Numlock",
        ScrollLock => "ScrollLock",
        PrintScreen => "PrintScreen",
        Menu => "Application",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::controls::scancode_of;

    #[test]
    fn every_capturable_key_is_a_name_the_core_knows() {
        use EKey::*;
        let keys = [
            A, Z, Num0, Num9, Minus, Equals, OpenBracket, CloseBracket, Backslash, Semicolon,
            Quote, Backtick, Comma, Period, Slash, Enter, Space, Tab, Backspace, Escape, Insert,
            Delete, Home, End, PageUp, PageDown, ArrowLeft, ArrowRight, ArrowUp, ArrowDown, F1,
            F12, ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight, SuperLeft,
            SuperRight, CapsLock, NumLock, ScrollLock, PrintScreen, Menu,
        ];
        for key in keys {
            let name = key_name(key).unwrap_or_else(|| panic!("{key:?} has no name"));
            assert!(scancode_of(name).is_some(), "{key:?} -> {name} is not a scancode name");
        }
    }

    #[test]
    fn unbindable_keys_are_ignored() {
        assert_eq!(key_name(EKey::F24), None, "F24 is the unbound marker");
        assert_eq!(key_name(EKey::Copy), None);
        assert_eq!(key_name(EKey::BrowserBack), None);
    }

    #[test]
    fn the_default_layout_shows_in_the_rows() {
        let settings = GameSettings::default();
        let rows = build_rows(&settings, Language::English, None);
        assert_eq!(rows.len(), 1 + 26 + 1, "hint, 26 actions, reset");
        assert_eq!(rows[0].id, "hint");
        let cross = &rows[1];
        assert_eq!(cross.id, "Cross");
        let rows::Control::Keys { sources, capturing } = &cross.control else {
            panic!("keys row");
        };
        assert_eq!(sources, &["KEY:K", "KEY:Space", "KEY:Return"]);
        assert!(!capturing);
        assert_eq!(rows.last().unwrap().id, "reset");
    }

    #[test]
    fn a_captured_key_replaces_keys_keeps_mouse_and_leaves_other_actions() {
        let mut settings = GameSettings::default();
        // Square has J and the left mouse button; give it K (Cross's key).
        let next = Controls::assign("KEY:K", PadAction::Square, &Default::default());
        settings.controls = Some(next);
        let rows = build_rows(&settings, Language::English, Some(PadAction::Square));
        let square = rows.iter().find(|r| r.id == "Square").unwrap();
        let rows::Control::Keys { sources, capturing } = &square.control else {
            panic!("keys row");
        };
        assert_eq!(sources, &["KEY:K", "MOUSE:Left"]);
        assert!(capturing);
        let cross = rows.iter().find(|r| r.id == "Cross").unwrap();
        let rows::Control::Keys { sources, .. } = &cross.control else {
            panic!("keys row");
        };
        assert_eq!(sources, &["KEY:Space", "KEY:Return"], "K was taken from Cross");
    }

    #[test]
    fn titles_localise_the_directions() {
        assert_eq!(action_title(PadAction::Up, Language::English), "D-pad · Up");
        assert_eq!(action_title(PadAction::Cross, Language::Turkish), "Cross");
        assert!(action_title(PadAction::LeftStickLeft, Language::English).starts_with("Left Stick"));
    }
}
