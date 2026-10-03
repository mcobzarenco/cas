//! A scriptable test rig: drives the running app from a tiny script so the UI can be exercised
//! and screenshotted without a human in the loop, e.g.
//!
//! ```text
//! cas --script "wait 30; shot start; click PlayPause; wait 60; shot running; quit"
//! ```
//!
//! Commands are separated by `;` or newlines; `#` starts a comment.
//!
//! | command             | effect                                                         |
//! |---------------------|----------------------------------------------------------------|
//! | `wait N`            | idle for N frames                                              |
//! | `shot NAME`         | save `<shots dir>/NAME.png` and wait until it is written        |
//! | `film NAME FRAMES [N]` | FRAMES screenshots `NAME-000.png`, `NAME-001.png`, ..., each |
//! |                     | followed by a step of N generations (1 unless given)           |
//! | `click NAME [DX DY]`| move the pointer to the UI node named NAME (plus an offset in  |
//! |                     | logical pixels from its centre), press, release                |
//! | `move NAME [DX DY]` | just move the pointer there                                    |
//! | `drag NAME DX DY [left/right/middle]` | press at the node's centre, move by (DX, DY) |
//! |                     | in a few steps, release                                        |
//! | `hold NAME FRAMES`  | press the node, keep the button down for FRAMES frames, release |
//! | `scroll NAME LINES` | turn the wheel over the node (positive is "up")                |
//! | `key KEY`           | press and release a key or chord: `Space`, `ArrowLeft`, `r`,   |
//! |                     | `[`, `Ctrl+a`, ...                                             |
//! | `press KEY`, `release KEY` | hold a key down across other commands, e.g. `Shift`; or |
//! |                     | a mouse button (`left`, `right`, `middle`) where the pointer is |
//! | `type TEXT`         | type text into whatever has keyboard focus                     |
//! | `clipboard TEXT`    | put text on the clipboard, as copying it elsewhere would: with |
//! |                     | `key Ctrl+v` a long text goes into a field in one go           |
//! | `paint X Y [on/off]`| set a cell directly                                            |
//! | `place RLE X Y`     | put a run-length encoded pattern with its corner at (X, Y)     |
//! | `fit`               | fit the view to the grid                                       |
//! | `window W H`        | give the window another size, in logical pixels                |
//! | `play`, `pause`     | transport                                                      |
//! | `step N`            | advance N generations (negative goes backwards)                |
//! | `rule RULE`         | switch rule: a preset name or a table of 16 states             |
//! | `speed N`           | frames per second                                              |
//! | `stride N`          | generations per frame                                          |
//! | `vacuum on/off`     | hide vacuum fluctuations                                       |
//! | `reverse on/off`    | run backwards                                                  |
//! | `soup [DENSITY]`    | uniform random soup                                            |
//! | `blob [DENSITY]`    | random square in the middle                                    |
//! | `cloud [DENSITY]`   | random cloud about the middle, thinning out                    |
//! | `expect_gen N`      | fail (exit code 1) unless the generation counter is N           |
//! | `expect_cell X Y on/off` | fail unless the cell has that state (as part of the       |
//! |                     | pattern: the vacuum is not counted in)                         |
//! | `expect_rule RULE`  | fail unless that rule is active                                |
//! | `expect_speed N`, `expect_stride N`, `expect_playing on/off` | likewise for the transport |
//! | `expect_population N` | fail unless the pattern has that many cells                  |
//! | `expect_size W H`   | fail unless the grid is W cells wide and H high                |
//! | `expect_checked NAME on/off` | fail unless the checkbox named NAME shows that state   |
//! | `expect_shown NAME on/off` | fail unless the node named NAME is on display (or is not:  |
//! |                     | a closed panel and all that is in it are not)                  |
//! | `expect_text NAME TEXT` | fail unless the node named NAME reads TEXT (a button, by  |
//! |                     | its caption)                                                   |
//! | `expect_caught SHIPS KINDS` | fail unless the catcher has that many spaceships, of that many kinds |
//! | `expect_clipboard TEXT` | fail unless the clipboard holds TEXT                       |
//! | `until EXPECTATION` | try the expectation at every frame until it holds, as in       |
//! |                     | `until expect_text Note Done.`: for what is done on another    |
//! |                     | thread, or by the clock. Fails if it never does                 |
//! | `clear`, `quit`     |                                                                |
//!
//! Pointer and keyboard actions are injected as the messages `bevy_winit` would produce, so they
//! flow through picking, focus and the widgets exactly like real input: clicking buttons,
//! dragging sliders, painting, wheel-zooming and panning the grid, opening menus and typing into
//! text fields are all scriptable. The window's own cursor position is deliberately left alone,
//! because changing it makes winit warp the real OS cursor.
//!
//! A run must not depend on what the person at the machine happens to do, so real input is
//! discarded while a script runs (and `main` makes the window transparent to the pointer).
//! A command that names a missing UI node or a cell outside the grid fails the run, and so
//! does a pointer command on a node that is not on display.

use std::{collections::VecDeque, path::PathBuf};

use bevy::{
    clipboard::Clipboard,
    ecs::{
        message::{MessageUpdateSystems, Messages},
        system::SystemParam,
    },
    input::{
        ButtonState,
        keyboard::{Key, KeyboardFocusLost, KeyboardInput, NativeKey},
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
        touch::TouchPhase,
    },
    picking::PickingSystems,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    text::EditableText,
    ui::{Checked, UiGlobalTransform},
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};

use cas_core::{
    pattern::{Cell, from_rle},
    rules::BlockRule,
    universe::{Rng, Universe},
};

use crate::{
    catcher::Catcher,
    sim::{Playback, Settings},
    view::ViewState,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Wait(u32),
    Shot(String),
    Click {
        name: String,
        offset: Vec2,
    },
    Move {
        name: String,
        offset: Vec2,
    },
    Drag {
        name: String,
        delta: Vec2,
        button: MouseButton,
    },
    Scroll {
        name: String,
        lines: f32,
    },
    Hold {
        name: String,
        frames: u32,
    },
    /// Keys pressed in order and released in reverse: `[ControlLeft, KeyA]` is Ctrl+A.
    Key(Vec<KeyCode>),
    Press(KeyCode),
    Release(KeyCode),
    /// A mouse button, where the pointer was last put.
    PressButton(MouseButton),
    ReleaseButton(MouseButton),
    Type(String),
    Paint {
        x: usize,
        y: usize,
        alive: bool,
    },
    Place {
        cells: Vec<Cell>,
        x: i32,
        y: i32,
    },
    Play,
    Pause,
    Step(i64),
    Rule(BlockRule),
    Speed(f32),
    Stride(u32),
    Vacuum(bool),
    Reverse(bool),
    Soup(Option<f32>),
    Cloud(Option<f32>),
    Blob(Option<f32>),
    Clear,
    Fit,
    /// The window's new size, in logical pixels.
    Window(u32, u32),
    ExpectGeneration(i64),
    ExpectCell {
        x: usize,
        y: usize,
        alive: bool,
    },
    ExpectRule(BlockRule),
    ExpectSpeed(f32),
    ExpectStride(u32),
    ExpectPopulation(usize),
    ExpectSize(usize, usize),
    ExpectPlaying(bool),
    ExpectChecked {
        name: String,
        checked: bool,
    },
    ExpectShown {
        name: String,
        shown: bool,
    },
    ExpectText {
        name: String,
        text: String,
    },
    ExpectCaught {
        ships: u64,
        kinds: usize,
    },
    ExpectClipboard(String),
    /// Text put on the clipboard, as copying it somewhere else would.
    Clipboard(String),
    /// An expectation that is waited for: tried again at every frame, for so many frames.
    Until {
        expectation: Box<Command>,
        frames: u32,
    },
    Quit,
}

/// For how many frames an expectation is waited for: a minute, at sixty frames a second.
const UNTIL_FRAMES: u32 = 3600;

impl Command {
    fn is_expectation(&self) -> bool {
        matches!(
            self,
            Command::ExpectGeneration(_)
                | Command::ExpectCell { .. }
                | Command::ExpectRule(_)
                | Command::ExpectSpeed(_)
                | Command::ExpectStride(_)
                | Command::ExpectPopulation(_)
                | Command::ExpectSize(..)
                | Command::ExpectPlaying(_)
                | Command::ExpectChecked { .. }
                | Command::ExpectShown { .. }
                | Command::ExpectText { .. }
                | Command::ExpectCaught { .. }
                | Command::ExpectClipboard(_)
        )
    }
}

pub fn parse_script(script: &str) -> Result<Vec<Command>, String> {
    let mut commands = Vec::new();
    let statements =
        script.lines().map(|line| line.split('#').next().unwrap_or_default()).flat_map(|line| line.split(';'));
    for raw in statements {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or_default();
        let args: Vec<&str> = words.collect();
        let arg = |i: usize, what: &str| -> Result<&str, String> {
            args.get(i).copied().ok_or_else(|| format!("`{command}` needs {what}"))
        };
        // Everything after the command, for arguments that may contain spaces.
        let rest = |what: &str| -> Result<String, String> {
            if args.is_empty() { Err(format!("`{command}` needs {what}")) } else { Ok(args.join(" ")) }
        };
        // What is waited for is an expectation, written as it would be on its own.
        if command == "until" {
            let mut waited = parse_script(&rest("an expectation")?)?;
            match (waited.pop(), waited.is_empty()) {
                (Some(expectation), true) if expectation.is_expectation() => {
                    commands.push(Command::Until { expectation: Box::new(expectation), frames: UNTIL_FRAMES });
                }
                _ => return Err(format!("`until` takes an expectation, found {line:?}")),
            }
            continue;
        }
        // A film is its frames: a screenshot, then the step to the next one.
        if command == "film" {
            let name = arg(0, "a file name")?;
            let frames: u32 = parse(arg(1, "a number of frames")?)?;
            let step = args.get(2).map_or(Ok(1), |s| parse(s))?;
            for frame in 0..frames {
                commands.extend([Command::Shot(format!("{name}-{frame:03}")), Command::Step(step)]);
            }
            continue;
        }
        let parsed = match command {
            "wait" => Command::Wait(parse(arg(0, "a frame count")?)?),
            "shot" => Command::Shot(arg(0, "a file name")?.to_string()),
            "click" => Command::Click { name: arg(0, "a UI node name")?.to_string(), offset: parse_offset(&args, 1)? },
            "move" => Command::Move { name: arg(0, "a UI node name")?.to_string(), offset: parse_offset(&args, 1)? },
            "drag" => Command::Drag {
                name: arg(0, "a UI node name")?.to_string(),
                delta: Vec2::new(parse(arg(1, "dx")?)?, parse(arg(2, "dy")?)?),
                button: args.get(3).map_or(Ok(MouseButton::Left), |s| parse_button(s))?,
            },
            "hold" => {
                Command::Hold { name: arg(0, "a UI node name")?.to_string(), frames: parse(arg(1, "a frame count")?)? }
            }
            "scroll" => Command::Scroll {
                name: arg(0, "a UI node name")?.to_string(),
                lines: parse(arg(1, "a number of wheel notches")?)?,
            },
            "key" => Command::Key(parse_chord(arg(0, "a key")?)?),
            "press" => match parse_button(arg(0, "a key or button")?) {
                Ok(button) => Command::PressButton(button),
                Err(_) => Command::Press(parse_key(arg(0, "a key")?)?),
            },
            "release" => match parse_button(arg(0, "a key or button")?) {
                Ok(button) => Command::ReleaseButton(button),
                Err(_) => Command::Release(parse_key(arg(0, "a key")?)?),
            },
            "type" => {
                let text = rest("some text")?;
                if let Some(c) = text.chars().find(|&c| key_for_char(c).is_none()) {
                    return Err(format!("`type` cannot type {c:?}"));
                }
                Command::Type(text)
            }
            "paint" => Command::Paint {
                x: parse(arg(0, "x")?)?,
                y: parse(arg(1, "y")?)?,
                alive: args.get(2).map_or(Ok(true), |s| parse_bool(s))?,
            },
            "place" => Command::Place {
                cells: from_rle(arg(0, "a run-length encoded pattern")?)?,
                x: parse(arg(1, "x")?)?,
                y: parse(arg(2, "y")?)?,
            },
            "play" => Command::Play,
            "pause" => Command::Pause,
            "step" => Command::Step(args.first().map_or(Ok(1), |s| parse(s))?),
            "rule" => Command::Rule(rest("a rule")?.parse()?),
            "speed" => Command::Speed(parse(arg(0, "frames per second")?)?),
            "stride" => Command::Stride(parse(arg(0, "generations per frame")?)?),
            "vacuum" => Command::Vacuum(parse_bool(arg(0, "on/off")?)?),
            "reverse" => Command::Reverse(parse_bool(arg(0, "on/off")?)?),
            "soup" => Command::Soup(args.first().map(|s| parse(s)).transpose()?),
            "blob" => Command::Blob(args.first().map(|s| parse(s)).transpose()?),
            "cloud" => Command::Cloud(args.first().map(|s| parse(s)).transpose()?),
            "clear" => Command::Clear,
            "fit" => Command::Fit,
            "window" => Command::Window(parse(arg(0, "a width")?)?, parse(arg(1, "a height")?)?),
            "expect_gen" => Command::ExpectGeneration(parse(arg(0, "a generation")?)?),
            "expect_cell" => Command::ExpectCell {
                x: parse(arg(0, "x")?)?,
                y: parse(arg(1, "y")?)?,
                alive: parse_bool(arg(2, "on/off")?)?,
            },
            "expect_rule" => Command::ExpectRule(rest("a rule")?.parse()?),
            "expect_speed" => Command::ExpectSpeed(parse(arg(0, "frames per second")?)?),
            "expect_stride" => Command::ExpectStride(parse(arg(0, "generations per frame")?)?),
            "expect_population" => Command::ExpectPopulation(parse(arg(0, "a number of cells")?)?),
            "expect_size" => Command::ExpectSize(parse(arg(0, "a width")?)?, parse(arg(1, "a height")?)?),
            "expect_playing" => Command::ExpectPlaying(parse_bool(arg(0, "on/off")?)?),
            "expect_checked" => Command::ExpectChecked {
                name: arg(0, "a UI node name")?.to_string(),
                checked: parse_bool(arg(1, "on/off")?)?,
            },
            "expect_shown" => Command::ExpectShown {
                name: arg(0, "a UI node name")?.to_string(),
                shown: parse_bool(arg(1, "on/off")?)?,
            },
            "expect_text" => {
                Command::ExpectText { name: arg(0, "a UI node name")?.to_string(), text: args[1..].join(" ") }
            }
            "expect_caught" => Command::ExpectCaught {
                ships: parse(arg(0, "a number of spaceships")?)?,
                kinds: parse(arg(1, "a number of kinds")?)?,
            },
            "expect_clipboard" => Command::ExpectClipboard(rest("some text")?),
            "clipboard" => Command::Clipboard(rest("some text")?),
            "quit" => Command::Quit,
            other => return Err(format!("unknown command `{other}` in {line:?}")),
        };
        commands.push(parsed);
    }
    Ok(commands)
}

fn parse<T: std::str::FromStr>(s: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    s.parse().map_err(|e| format!("{s:?}: {e}"))
}

/// An optional `DX DY` pair starting at `args[at]`.
fn parse_offset(args: &[&str], at: usize) -> Result<Vec2, String> {
    match args.get(at..) {
        None | Some([]) => Ok(Vec2::ZERO),
        Some([dx, dy, ..]) => Ok(Vec2::new(parse(dx)?, parse(dy)?)),
        Some([_]) => Err("an offset needs both DX and DY".into()),
    }
}

fn parse_button(s: &str) -> Result<MouseButton, String> {
    match s {
        "left" => Ok(MouseButton::Left),
        "right" => Ok(MouseButton::Right),
        "middle" => Ok(MouseButton::Middle),
        _ => Err(format!("{s:?} is not left/right/middle")),
    }
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s {
        "1" | "on" | "true" | "yes" => Ok(true),
        "0" | "off" | "false" | "no" => Ok(false),
        _ => Err(format!("{s:?} is not on/off")),
    }
}

/// Keys that produce a character, with the character they produce (unshifted).
const CHARACTER_KEYS: [(char, KeyCode); 44] = [
    ('a', KeyCode::KeyA),
    ('b', KeyCode::KeyB),
    ('c', KeyCode::KeyC),
    ('d', KeyCode::KeyD),
    ('e', KeyCode::KeyE),
    ('f', KeyCode::KeyF),
    ('g', KeyCode::KeyG),
    ('h', KeyCode::KeyH),
    ('i', KeyCode::KeyI),
    ('j', KeyCode::KeyJ),
    ('k', KeyCode::KeyK),
    ('l', KeyCode::KeyL),
    ('m', KeyCode::KeyM),
    ('n', KeyCode::KeyN),
    ('o', KeyCode::KeyO),
    ('p', KeyCode::KeyP),
    ('q', KeyCode::KeyQ),
    ('r', KeyCode::KeyR),
    ('s', KeyCode::KeyS),
    ('t', KeyCode::KeyT),
    ('u', KeyCode::KeyU),
    ('v', KeyCode::KeyV),
    ('w', KeyCode::KeyW),
    ('x', KeyCode::KeyX),
    ('y', KeyCode::KeyY),
    ('z', KeyCode::KeyZ),
    ('0', KeyCode::Digit0),
    ('1', KeyCode::Digit1),
    ('2', KeyCode::Digit2),
    ('3', KeyCode::Digit3),
    ('4', KeyCode::Digit4),
    ('5', KeyCode::Digit5),
    ('6', KeyCode::Digit6),
    ('7', KeyCode::Digit7),
    ('8', KeyCode::Digit8),
    ('9', KeyCode::Digit9),
    ('[', KeyCode::BracketLeft),
    (']', KeyCode::BracketRight),
    (',', KeyCode::Comma),
    ('.', KeyCode::Period),
    ('=', KeyCode::Equal),
    ('-', KeyCode::Minus),
    // The end of a row in a pattern's text; on the key of the 4, where many keyboards have it.
    ('$', KeyCode::Digit4),
    (' ', KeyCode::Space),
];

fn key_for_char(c: char) -> Option<KeyCode> {
    let c = c.to_ascii_lowercase();
    CHARACTER_KEYS.iter().find(|(character, _)| *character == c).map(|(_, code)| *code)
}

fn parse_key(s: &str) -> Result<KeyCode, String> {
    let named = match s {
        "Space" | "space" => Some(KeyCode::Space),
        "ArrowLeft" | "Left" => Some(KeyCode::ArrowLeft),
        "ArrowRight" | "Right" => Some(KeyCode::ArrowRight),
        "ArrowUp" | "Up" => Some(KeyCode::ArrowUp),
        "ArrowDown" | "Down" => Some(KeyCode::ArrowDown),
        "Enter" => Some(KeyCode::Enter),
        "Escape" => Some(KeyCode::Escape),
        "Tab" => Some(KeyCode::Tab),
        "Backspace" => Some(KeyCode::Backspace),
        "Delete" => Some(KeyCode::Delete),
        "Home" => Some(KeyCode::Home),
        "End" => Some(KeyCode::End),
        "Ctrl" | "Control" => Some(KeyCode::ControlLeft),
        "Shift" => Some(KeyCode::ShiftLeft),
        "Alt" => Some(KeyCode::AltLeft),
        _ => None,
    };
    if let Some(code) = named {
        return Ok(code);
    }
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c != ' '
        && let Some(code) = key_for_char(c)
    {
        return Ok(code);
    }
    Err(format!("unknown key {s:?}"))
}

/// `Ctrl+a` → `[ControlLeft, KeyA]`.
fn parse_chord(s: &str) -> Result<Vec<KeyCode>, String> {
    s.split('+').map(parse_key).collect()
}

/// Best-effort logical key for a synthetic key code.
fn logical_key(code: KeyCode) -> Key {
    match code {
        KeyCode::Space => Key::Space,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::ControlLeft => Key::Control,
        KeyCode::ShiftLeft => Key::Shift,
        KeyCode::AltLeft => Key::Alt,
        _ => CHARACTER_KEYS
            .iter()
            .find(|(_, k)| *k == code)
            .map_or(Key::Unidentified(NativeKey::Unidentified), |(c, _)| Key::Character(c.to_string().into())),
    }
}

/// One frame's worth of synthetic input; picking and focus want to see each step separately.
#[derive(Clone, Copy, Debug)]
enum InputStep {
    /// Move the pointer to a position in logical window coordinates.
    Move(Vec2),
    /// Press or release a button at a position. The position is re-asserted in the same frame,
    /// so a real mouse wandering over the window cannot redirect the click.
    Button(Vec2, MouseButton, ButtonState),
    Wheel(f32),
    Key(KeyCode, ButtonState),
    /// Press and release a key that types a character.
    Type(char),
    /// Let a frame pass (while a button is held).
    Idle,
}

/// How many pointer moves a `drag` is spread over.
const DRAG_STEPS: u32 = 8;

#[derive(Resource)]
struct Rig {
    script: VecDeque<Command>,
    dir: PathBuf,
    /// Frames to idle before the first command, so the window and the layout can settle.
    settle: u32,
    wait: u32,
    input: VecDeque<InputStep>,
    /// Where the pointer was last sent, for a button pressed without a node to press it on.
    pointer: Vec2,
    shot: Option<Entity>,
}

pub struct RigPlugin {
    pub script: Vec<Command>,
    pub dir: PathBuf,
}

impl Plugin for RigPlugin {
    fn build(&self, app: &mut App) {
        std::fs::create_dir_all(&self.dir).unwrap_or_else(|e| panic!("cannot create {}: {e}", self.dir.display()));
        app.insert_resource(Rig {
            script: self.script.clone().into(),
            dir: self.dir.clone(),
            settle: 15,
            wait: 0,
            input: VecDeque::new(),
            pointer: Vec2::ZERO,
            shot: None,
        })
        // Before anything reads this frame's input: picking does so in `First` already.
        .add_systems(First, drive.after(MessageUpdateSystems).before(PickingSystems::Input));
    }
}

/// The input messages of the app. `bevy_winit` writes every event both as its own message and
/// wrapped in a `WindowEvent`, and so does the rig.
#[derive(SystemParam)]
struct Input<'w, 's> {
    window: Single<'w, 's, Entity, With<PrimaryWindow>>,
    cursor_moved: ResMut<'w, Messages<CursorMoved>>,
    mouse_buttons: ResMut<'w, Messages<MouseButtonInput>>,
    mouse_wheel: ResMut<'w, Messages<MouseWheel>>,
    keyboard: ResMut<'w, Messages<KeyboardInput>>,
    focus_lost: ResMut<'w, Messages<KeyboardFocusLost>>,
    window_events: ResMut<'w, Messages<WindowEvent>>,
}

impl Input<'_, '_> {
    /// Throws away what the real mouse and keyboard did since the last frame.
    fn discard_real(&mut self) {
        self.cursor_moved.clear();
        self.mouse_buttons.clear();
        self.mouse_wheel.clear();
        self.keyboard.clear();
        self.focus_lost.clear();
        let others: Vec<WindowEvent> = self
            .window_events
            .drain()
            .filter(|event| {
                !matches!(
                    event,
                    WindowEvent::CursorMoved(_)
                        | WindowEvent::MouseButtonInput(_)
                        | WindowEvent::MouseWheel(_)
                        | WindowEvent::KeyboardInput(_)
                        | WindowEvent::KeyboardFocusLost(_)
                )
            })
            .collect();
        self.window_events.write_batch(others);
    }

    fn send(&mut self, step: InputStep) {
        let window = *self.window;
        match step {
            InputStep::Move(position) => {
                let event = CursorMoved { window, position, delta: None };
                self.cursor_moved.write(event.clone());
                self.window_events.write(WindowEvent::CursorMoved(event));
            }
            InputStep::Button(position, button, state) => {
                self.send(InputStep::Move(position));
                let event = MouseButtonInput { button, state, window };
                self.mouse_buttons.write(event);
                self.window_events.write(WindowEvent::MouseButtonInput(event));
            }
            InputStep::Wheel(lines) => {
                let event =
                    MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y: lines, window, phase: TouchPhase::Moved };
                self.mouse_wheel.write(event);
                self.window_events.write(WindowEvent::MouseWheel(event));
            }
            InputStep::Key(code, state) => self.key(code, logical_key(code), state, None),
            InputStep::Type(c) => {
                let code = key_for_char(c).unwrap_or(KeyCode::Space);
                let logical = Key::Character(c.to_string().into());
                self.key(code, logical.clone(), ButtonState::Pressed, Some(c));
                self.key(code, logical, ButtonState::Released, None);
            }
            InputStep::Idle => {}
        }
    }

    fn key(&mut self, code: KeyCode, logical_key: Key, state: ButtonState, text: Option<char>) {
        let event = KeyboardInput {
            key_code: code,
            logical_key,
            state,
            text: text.map(|c| c.to_string().into()),
            repeat: false,
            window: *self.window,
        };
        self.keyboard.write(event.clone());
        self.window_events.write(WindowEvent::KeyboardInput(event));
    }
}

/// What a node of the interface reads as: its own text with the spans that carry it on, what
/// is in it if it is a text field, or, for a node without text of its own, the texts in it.
#[derive(SystemParam)]
struct Reading<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    texts: Query<'w, 's, &'static Text>,
    spans: Query<'w, 's, &'static TextSpan>,
    fields: Query<'w, 's, &'static EditableText>,
}

impl Reading<'_, '_> {
    fn of(&self, entity: Entity) -> String {
        if let Ok(field) = self.fields.get(entity) {
            return field.value().to_string();
        }
        // A text reads on through the spans that carry it on in another face or colour.
        let text = |entity: Entity| {
            let text = self.texts.get(entity).ok()?;
            let carried = self.children.get(entity).into_iter().flatten();
            let carried = carried.filter_map(|&span| self.spans.get(span).ok());
            Some(carried.fold(text.to_string(), |read, span| read + &span.0))
        };
        // A node without text of its own, such as a button, reads as what is written in it.
        let read = text(entity).unwrap_or_else(|| {
            let within = self.children.iter_descendants(entity).filter_map(text);
            within.collect::<Vec<_>>().join(" ")
        });
        // Space is space, of whatever kind and however much: a script has single spaces.
        read.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// Runs the script: one input step or one command per frame.
fn drive(
    mut rig: ResMut<Rig>,
    mut input: Input,
    nodes: Query<(Entity, &Name, &ComputedNode, &UiGlobalTransform, Has<Checked>)>,
    reading: Reading,
    screenshots: Query<(), With<Screenshot>>,
    mut app_exit: MessageWriter<AppExit>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
    mut universe: ResMut<Universe>,
    mut view: ResMut<ViewState>,
    mut rng: ResMut<Rng>,
    mut clipboard: ResMut<Clipboard>,
    catcher: Res<Catcher>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    input.discard_real();
    if rig.settle > 0 {
        rig.settle -= 1;
        return;
    }
    if let Some(shot) = rig.shot {
        // The screenshot entity is despawned once the capture has been handed to `save_to_disk`.
        if screenshots.get(shot).is_ok() {
            return;
        }
        rig.shot = None;
        rig.wait = 2;
    }
    if rig.wait > 0 {
        rig.wait -= 1;
        return;
    }
    if let Some(step) = rig.input.pop_front() {
        input.send(step);
        return;
    }
    let Some(command) = rig.script.pop_front() else {
        return;
    };
    // An expectation that is waited for is tried now, and again next frame if it fails.
    let (command, waited) = match command {
        Command::Until { expectation, frames } => (*expectation, Some(frames)),
        command => (command, None),
    };
    match waited {
        None => info!("rig: {command:?}"),
        Some(UNTIL_FRAMES) => info!("rig: until {command:?}"),
        Some(_) => {}
    }
    let again = waited.map(|_| command.clone());

    let node = |name: &str| {
        nodes.iter().find(|(_, n, ..)| n.as_str() == name).ok_or_else(|| format!("no UI node named {name:?}"))
    };
    // A node takes up room unless it, or what it is in, is not displayed.
    let shown = |computed: &ComputedNode| computed.size.min_element() > 0.0;
    // Centre of a named UI node in logical window coordinates. A node that is not on display
    // has none: the pointer has nothing to do there.
    let locate = |name: &str| {
        let (_, _, computed, transform, ..) = node(name)?;
        if !shown(computed) {
            return Err(format!("the UI node named {name:?} is not on display"));
        }
        Ok(transform.translation * computed.inverse_scale_factor)
    };
    let expect = |holds: bool, complaint: String| if holds { Ok(()) } else { Err(complaint) };
    use ButtonState::{Pressed, Released};

    let outcome: Result<(), String> = (|| {
        match command {
            Command::Wait(frames) => rig.wait = frames,
            Command::Shot(name) => {
                let path = rig.dir.join(format!("{name}.png"));
                let entity = commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path)).id();
                rig.shot = Some(entity);
            }
            Command::Click { name, offset } => {
                let at = locate(&name)? + offset;
                rig.input.extend([
                    InputStep::Move(at),
                    InputStep::Button(at, MouseButton::Left, Pressed),
                    InputStep::Button(at, MouseButton::Left, Released),
                ]);
            }
            Command::Move { name, offset } => {
                let at = locate(&name)? + offset;
                rig.pointer = at;
                rig.input.push_back(InputStep::Move(at));
            }
            Command::Drag { name, delta, button } => {
                let center = locate(&name)?;
                rig.input.push_back(InputStep::Move(center));
                rig.input.push_back(InputStep::Button(center, button, Pressed));
                for i in 1..=DRAG_STEPS {
                    let along = delta * (i as f32 / DRAG_STEPS as f32);
                    rig.input.push_back(InputStep::Move(center + along));
                }
                rig.input.push_back(InputStep::Button(center + delta, button, Released));
            }
            Command::Hold { name, frames } => {
                let center = locate(&name)?;
                rig.input.push_back(InputStep::Move(center));
                rig.input.push_back(InputStep::Button(center, MouseButton::Left, Pressed));
                rig.input.extend(std::iter::repeat_n(InputStep::Idle, frames as usize));
                rig.input.push_back(InputStep::Button(center, MouseButton::Left, Released));
            }
            Command::Scroll { name, lines } => {
                let center = locate(&name)?;
                rig.input.extend([InputStep::Move(center), InputStep::Move(center), InputStep::Wheel(lines)]);
            }
            Command::Key(chord) => {
                rig.input.extend(chord.iter().map(|&code| InputStep::Key(code, Pressed)));
                rig.input.extend(chord.iter().rev().map(|&code| InputStep::Key(code, Released)));
                rig.input.push_back(InputStep::Idle);
            }
            Command::Press(code) => rig.input.push_back(InputStep::Key(code, Pressed)),
            Command::Release(code) => rig.input.push_back(InputStep::Key(code, Released)),
            Command::PressButton(button) => {
                let at = rig.pointer;
                rig.input.push_back(InputStep::Button(at, button, Pressed));
            }
            Command::ReleaseButton(button) => {
                let at = rig.pointer;
                rig.input.push_back(InputStep::Button(at, button, Released));
            }
            Command::Type(text) => {
                rig.input.extend(text.chars().map(InputStep::Type));
                rig.input.push_back(InputStep::Idle);
            }
            Command::Paint { x, y, alive } => {
                expect(x < universe.width && y < universe.height, format!("cell ({x}, {y}) is outside the grid"))?;
                universe.set(x, y, alive);
            }
            Command::Place { cells, x, y } => {
                let on_grid = |&(cx, cy): &Cell| {
                    (0..universe.width as i32).contains(&(x + cx)) && (0..universe.height as i32).contains(&(y + cy))
                };
                expect(cells.iter().all(on_grid), "the pattern does not fit on the grid".into())?;
                for (cx, cy) in cells {
                    universe.set((x + cx) as usize, (y + cy) as usize, true);
                }
            }
            Command::Play => playback.playing = true,
            Command::Pause => playback.playing = false,
            Command::Step(steps) => universe.step_by(steps),
            Command::Rule(rule) => universe.set_rule(rule),
            Command::Speed(speed) => {
                playback.speed = speed.clamp(Playback::MIN_SPEED, Playback::MAX_SPEED);
            }
            Command::Stride(stride) => playback.stride = stride.clamp(1, Playback::MAX_STRIDE),
            Command::Vacuum(on) => settings.hide_vacuum = on,
            Command::Reverse(on) => playback.reverse = on,
            Command::Soup(density) => {
                let density = density.unwrap_or(settings.density);
                universe.randomize(density, &mut rng);
            }
            Command::Blob(density) => {
                let density = density.unwrap_or(settings.density);
                universe.randomize_blob(density, &mut rng);
            }
            Command::Cloud(density) => {
                let density = density.unwrap_or(settings.density);
                universe.randomize_cloud(density, &mut rng);
            }
            Command::Clear => universe.clear(),
            Command::Fit => view.fit = true,
            Command::Window(width, height) => {
                for mut window in &mut windows {
                    window.resolution.set(width as f32, height as f32);
                }
            }
            Command::ExpectGeneration(expected) => expect(
                universe.generation == expected,
                format!("expected generation {expected}, found {}", universe.generation),
            )?,
            Command::ExpectCell { x, y, alive } => {
                expect(x < universe.width && y < universe.height, format!("cell ({x}, {y}) is outside the grid"))?;
                expect(universe.get(x, y) == alive, format!("expected cell ({x}, {y}) to be {alive}"))?;
            }
            Command::ExpectRule(expected) => {
                expect(*universe.rule() == expected, format!("expected rule {expected}, found {}", universe.rule()))?
            }
            Command::ExpectSpeed(expected) => {
                expect(playback.speed == expected, format!("expected speed {expected}, found {}", playback.speed))?
            }
            Command::ExpectStride(expected) => {
                expect(playback.stride == expected, format!("expected stride {expected}, found {}", playback.stride))?
            }
            Command::ExpectPopulation(expected) => expect(
                universe.population() == expected,
                format!("expected population {expected}, found {}", universe.population()),
            )?,
            Command::ExpectSize(width, height) => expect(
                (universe.width, universe.height) == (width, height),
                format!("expected a grid of {width}×{height}, found {}×{}", universe.width, universe.height),
            )?,
            Command::ExpectPlaying(expected) => {
                expect(playback.playing == expected, format!("expected playing to be {expected}"))?
            }
            Command::ExpectChecked { name, checked } => {
                let (.., found) = node(&name)?;
                expect(found == checked, format!("expected {name} to be checked: {checked}"))?;
            }
            Command::ExpectShown { name, shown: expected } => {
                let (_, _, computed, ..) = node(&name)?;
                expect(shown(computed) == expected, format!("expected {name} to be on display: {expected}"))?;
            }
            Command::ExpectText { name, text } => {
                let (entity, ..) = node(&name)?;
                let found = reading.of(entity);
                expect(found == text, format!("expected {name} to read {text:?}, found {found:?}"))?;
            }
            Command::ExpectCaught { ships, kinds } => {
                let found = catcher.totals(universe.rule());
                expect(
                    found == (ships, kinds),
                    format!("expected {ships} spaceships of {kinds} kinds, found {found:?}"),
                )?;
            }
            Command::Clipboard(text) => {
                let copied = clipboard.set_text(text.as_str());
                copied.map_err(|error| format!("the clipboard is not available ({error:?})"))?;
            }
            Command::ExpectClipboard(expected) => {
                let found = clipboard.fetch_text().poll_result();
                expect(
                    matches!(&found, Some(Ok(text)) if *text == expected),
                    format!("expected clipboard {expected:?}, found {found:?}"),
                )?;
            }
            Command::Until { .. } => return Err("an expectation cannot be waited for twice over".into()),
            Command::Quit => {
                app_exit.write(AppExit::Success);
            }
        }
        Ok(())
    })();
    let outcome = match (outcome, waited, again) {
        (Err(_), Some(frames), Some(expectation)) if frames > 0 => {
            let expectation = Box::new(expectation);
            rig.script.push_front(Command::Until { expectation, frames: frames - 1 });
            Ok(())
        }
        (Err(complaint), Some(_), _) => Err(format!("{complaint}, however long it was waited for")),
        (outcome, ..) => outcome,
    };
    if let Err(complaint) = outcome {
        error!("rig: {complaint}");
        app_exit.write(AppExit::error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_script() {
        let script = parse_script(
            "wait 30; shot start\n click PlayPause; key Space; key [; step -3; rule critters; soup; soup 0.5; paint 3 4 off; quit\n\
             click Speed -40 0; drag Grid 30 -20 right; scroll Grid 3; expect_cell 1 2 on; fit\n\
             key Ctrl+a; type 0,2 x; rule 15,1,2,3,4,5,6,7,8,9,10,11,12,13,14,0; expect_rule Tron; expect_clipboard a b\n\
             clipboard 2o$b o",
        )
        .unwrap();
        assert_eq!(
            script,
            vec![
                Command::Wait(30),
                Command::Shot("start".into()),
                Command::Click { name: "PlayPause".into(), offset: Vec2::ZERO },
                Command::Key(vec![KeyCode::Space]),
                Command::Key(vec![KeyCode::BracketLeft]),
                Command::Step(-3),
                Command::Rule("critters".parse().unwrap()),
                Command::Soup(None),
                Command::Soup(Some(0.5)),
                Command::Paint { x: 3, y: 4, alive: false },
                Command::Quit,
                Command::Click { name: "Speed".into(), offset: Vec2::new(-40.0, 0.0) },
                Command::Drag { name: "Grid".into(), delta: Vec2::new(30.0, -20.0), button: MouseButton::Right },
                Command::Scroll { name: "Grid".into(), lines: 3.0 },
                Command::ExpectCell { x: 1, y: 2, alive: true },
                Command::Fit,
                Command::Key(vec![KeyCode::ControlLeft, KeyCode::KeyA]),
                Command::Type("0,2 x".into()),
                Command::Rule("tron".parse().unwrap()),
                Command::ExpectRule("tron".parse().unwrap()),
                Command::ExpectClipboard("a b".into()),
                Command::Clipboard("2o$b o".into()),
            ]
        );
    }

    #[test]
    fn comments_run_to_the_end_of_the_line() {
        let script = parse_script("# a comment; with a semicolon\nwait 1 # trailing; comment\nquit").unwrap();
        assert_eq!(script, vec![Command::Wait(1), Command::Quit]);
    }

    #[test]
    fn keys_can_be_held_and_state_checked() {
        let script =
            parse_script("press Shift; drag Grid 60 0; release Shift; expect_population 12; expect_playing off")
                .unwrap();
        assert_eq!(
            script,
            vec![
                Command::Press(KeyCode::ShiftLeft),
                Command::Drag { name: "Grid".into(), delta: Vec2::new(60.0, 0.0), button: MouseButton::Left },
                Command::Release(KeyCode::ShiftLeft),
                Command::ExpectPopulation(12),
                Command::ExpectPlaying(false),
            ]
        );
    }

    #[test]
    fn patterns_are_placed_and_the_world_and_its_catches_checked() {
        let script = parse_script(
            "place 2o$bo 3 4; expect_size 64 32; expect_caught 5 2; expect_text Note No ships: yet.; expect_text Note",
        )
        .unwrap();
        assert_eq!(
            script,
            vec![
                Command::Place { cells: vec![(0, 0), (1, 0), (1, 1)], x: 3, y: 4 },
                Command::ExpectSize(64, 32),
                Command::ExpectCaught { ships: 5, kinds: 2 },
                Command::ExpectText { name: "Note".into(), text: "No ships: yet.".into() },
                Command::ExpectText { name: "Note".into(), text: String::new() },
            ]
        );
        // A film is so many screenshots, each followed by a step.
        assert_eq!(
            parse_script("film ship 2 -3").unwrap(),
            vec![
                Command::Shot("ship-000".into()),
                Command::Step(-3),
                Command::Shot("ship-001".into()),
                Command::Step(-3),
            ]
        );
        assert_eq!(parse_script("film ship 1").unwrap()[1], Command::Step(1));
        assert!(parse_script("film ship").is_err());
        assert!(parse_script("place 2x 3 4").is_err());
        assert_eq!(
            parse_script("window 1280 720; expect_shown Catcher off").unwrap(),
            vec![Command::Window(1280, 720), Command::ExpectShown { name: "Catcher".into(), shown: false }]
        );
        assert!(parse_script("window 1280").is_err());
        // An expectation can be waited for; nothing else can.
        assert_eq!(
            parse_script("until expect_text Note Done.").unwrap(),
            vec![Command::Until {
                expectation: Box::new(Command::ExpectText { name: "Note".into(), text: "Done.".into() }),
                frames: UNTIL_FRAMES,
            }]
        );
        assert!(parse_script("until click Note").is_err() && parse_script("until").is_err());
        assert!(parse_script("expect_size 64").is_err());
        assert!(parse_script("expect_text").is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_script("wait").is_err());
        assert!(parse_script("frobnicate 1").is_err());
        assert!(parse_script("key F13").is_err());
        assert!(parse_script("rule life").is_err());
        assert!(parse_script("rule 0,0,0").is_err());
        assert!(parse_script("click PlayPause 4").is_err());
        assert!(parse_script("drag Grid 1 2 sideways").is_err());
        assert!(parse_script("type héllo").is_err());
    }
}
