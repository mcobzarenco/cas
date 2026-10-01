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
//! | `click NAME [DX DY]`| move the pointer to the UI node named NAME (plus an offset in  |
//! |                     | logical pixels from its centre), press, release                |
//! | `move NAME [DX DY]` | just move the pointer there                                    |
//! | `drag NAME DX DY [left/right/middle]` | press at the node's centre, move by (DX, DY) |
//! |                     | in a few steps, release                                        |
//! | `hold NAME FRAMES`  | press the node, keep the button down for FRAMES frames, release |
//! | `scroll NAME LINES` | turn the wheel over the node (positive zooms in)               |
//! | `key KEY`           | press and release a key (`Space`, `ArrowLeft`, `r`, `[`, ...)  |
//! | `paint X Y [on/off]`| set a cell directly                                            |
//! | `fit`               | fit the view to the grid                                       |
//! | `play`, `pause`     | transport                                                      |
//! | `step N`            | advance N generations (negative goes backwards)                |
//! | `rule NAME`         | switch rule                                                    |
//! | `speed N`           | frames per second                                              |
//! | `stride N`          | generations per frame                                          |
//! | `vacuum on/off`     | hide vacuum fluctuations                                       |
//! | `reverse on/off`    | run backwards                                                  |
//! | `soup [DENSITY]`    | uniform random soup                                            |
//! | `blob [DENSITY]`    | random square in the middle                                    |
//! | `expect_gen N`      | fail (exit code 1) unless the generation counter is N           |
//! | `expect_cell X Y on/off` | fail unless the cell has that state                       |
//! | `clear`, `quit`     |                                                                |
//!
//! Pointer and keyboard actions are injected as the messages `bevy_winit` would produce, so they
//! flow through picking, focus and the widgets exactly like real input: clicking buttons,
//! dragging sliders, painting, wheel-zooming and panning the grid are all scriptable. The
//! window's own cursor position is deliberately left alone, because changing it makes winit warp
//! the real OS cursor.

use std::{collections::VecDeque, path::PathBuf};

use bevy::{
    ecs::system::SystemParam,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
        touch::TouchPhase,
    },
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    ui::UiGlobalTransform,
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};

use crate::{
    rules::RuleKind,
    sim::{Playback, Rng, Settings, Universe},
    view::ViewState,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Wait(u32),
    Shot(String),
    Click { name: String, offset: Vec2 },
    Move { name: String, offset: Vec2 },
    Drag { name: String, delta: Vec2, button: MouseButton },
    Scroll { name: String, lines: f32 },
    Hold { name: String, frames: u32 },
    Key(KeyCode),
    Paint { x: usize, y: usize, alive: bool },
    Play,
    Pause,
    Step(i64),
    Rule(RuleKind),
    Speed(f32),
    Stride(u32),
    Vacuum(bool),
    Reverse(bool),
    Soup(Option<f32>),
    Blob(Option<f32>),
    Clear,
    Fit,
    ExpectGeneration(i64),
    ExpectCell { x: usize, y: usize, alive: bool },
    Quit,
}

pub fn parse_script(script: &str) -> Result<Vec<Action>, String> {
    let mut actions = Vec::new();
    let statements = script
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(|line| line.split(';'));
    for raw in statements {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or_default();
        let args: Vec<&str> = words.collect();
        let arg = |i: usize, what: &str| -> Result<&str, String> {
            args.get(i)
                .copied()
                .ok_or_else(|| format!("`{command}` needs {what}"))
        };
        let action = match command {
            "wait" => Action::Wait(parse(arg(0, "a frame count")?)?),
            "shot" => Action::Shot(arg(0, "a file name")?.to_string()),
            "click" => Action::Click {
                name: arg(0, "a UI node name")?.to_string(),
                offset: parse_offset(&args, 1)?,
            },
            "move" => Action::Move {
                name: arg(0, "a UI node name")?.to_string(),
                offset: parse_offset(&args, 1)?,
            },
            "drag" => Action::Drag {
                name: arg(0, "a UI node name")?.to_string(),
                delta: Vec2::new(parse(arg(1, "dx")?)?, parse(arg(2, "dy")?)?),
                button: args.get(3).map_or(Ok(MouseButton::Left), |s| parse_button(s))?,
            },
            "hold" => Action::Hold {
                name: arg(0, "a UI node name")?.to_string(),
                frames: parse(arg(1, "a frame count")?)?,
            },
            "scroll" => Action::Scroll {
                name: arg(0, "a UI node name")?.to_string(),
                lines: parse(arg(1, "a number of wheel notches")?)?,
            },
            "key" => Action::Key(parse_key(arg(0, "a key")?)?),
            "paint" => Action::Paint {
                x: parse(arg(0, "x")?)?,
                y: parse(arg(1, "y")?)?,
                alive: args.get(2).map_or(Ok(true), |s| parse_bool(s))?,
            },
            "play" => Action::Play,
            "pause" => Action::Pause,
            "step" => Action::Step(args.first().map_or(Ok(1), |s| parse(s))?),
            "rule" => Action::Rule(arg(0, "a rule name")?.parse()?),
            "speed" => Action::Speed(parse(arg(0, "frames per second")?)?),
            "stride" => Action::Stride(parse(arg(0, "generations per frame")?)?),
            "vacuum" => Action::Vacuum(parse_bool(arg(0, "on/off")?)?),
            "reverse" => Action::Reverse(parse_bool(arg(0, "on/off")?)?),
            "soup" => Action::Soup(args.first().map(|s| parse(s)).transpose()?),
            "blob" => Action::Blob(args.first().map(|s| parse(s)).transpose()?),
            "clear" => Action::Clear,
            "fit" => Action::Fit,
            "expect_gen" => Action::ExpectGeneration(parse(arg(0, "a generation")?)?),
            "expect_cell" => Action::ExpectCell {
                x: parse(arg(0, "x")?)?,
                y: parse(arg(1, "y")?)?,
                alive: parse_bool(arg(2, "on/off")?)?,
            },
            "quit" => Action::Quit,
            other => return Err(format!("unknown command `{other}` in {line:?}")),
        };
        actions.push(action);
    }
    Ok(actions)
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

const LETTERS: [(char, KeyCode); 26] = [
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
];

const PUNCTUATION: [(char, KeyCode); 6] = [
    ('[', KeyCode::BracketLeft),
    (']', KeyCode::BracketRight),
    (',', KeyCode::Comma),
    ('.', KeyCode::Period),
    ('=', KeyCode::Equal),
    ('-', KeyCode::Minus),
];

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
        _ => None,
    };
    if let Some(code) = named {
        return Ok(code);
    }
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let c = c.to_ascii_lowercase();
        if let Some((_, code)) = LETTERS.iter().chain(&PUNCTUATION).find(|(k, _)| *k == c) {
            return Ok(*code);
        }
    }
    Err(format!("unknown key {s:?}"))
}

/// Best-effort logical key for a synthetic key code (only the keys the app binds matter).
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
        _ => LETTERS
            .iter()
            .chain(&PUNCTUATION)
            .find(|(_, k)| *k == code)
            .map_or(Key::Unidentified(NativeKey::Unidentified), |(c, _)| {
                Key::Character(c.to_string().into())
            }),
    }
}

/// One frame's worth of synthetic pointer input; picking wants to see each step separately.
#[derive(Clone, Copy, Debug)]
enum PointerStep {
    /// Move to a position in logical window coordinates.
    Move(Vec2),
    Button(MouseButton, ButtonState),
    Wheel(f32),
    /// Let a frame pass (while a button is held).
    Idle,
}

/// How many pointer moves a `drag` is spread over.
const DRAG_STEPS: u32 = 8;

#[derive(Resource)]
struct Rig {
    actions: VecDeque<Action>,
    dir: PathBuf,
    /// Frames to idle before the first action, so the window and the layout can settle.
    settle: u32,
    wait: u32,
    pointer: VecDeque<PointerStep>,
    key_release: Option<KeyCode>,
    shot: Option<Entity>,
}

pub struct RigPlugin {
    pub actions: Vec<Action>,
    pub dir: PathBuf,
}

impl Plugin for RigPlugin {
    fn build(&self, app: &mut App) {
        std::fs::create_dir_all(&self.dir)
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", self.dir.display()));
        app.insert_resource(Rig {
            actions: self.actions.clone().into(),
            dir: self.dir.clone(),
            settle: 15,
            wait: 0,
            pointer: VecDeque::new(),
            key_release: None,
            shot: None,
        })
        .add_systems(First, drive);
    }
}

/// Writes input the way `bevy_winit` does: every event goes out both as its own message and
/// wrapped in a `WindowEvent`.
#[derive(SystemParam)]
struct Input<'w, 's> {
    window: Single<'w, 's, Entity, With<PrimaryWindow>>,
    cursor_moved: MessageWriter<'w, CursorMoved>,
    mouse_buttons: MessageWriter<'w, MouseButtonInput>,
    mouse_wheel: MessageWriter<'w, MouseWheel>,
    keyboard: MessageWriter<'w, KeyboardInput>,
    window_events: MessageWriter<'w, WindowEvent>,
}

impl Input<'_, '_> {
    fn pointer(&mut self, step: PointerStep) {
        let window = *self.window;
        match step {
            PointerStep::Move(position) => {
                let event = CursorMoved {
                    window,
                    position,
                    delta: None,
                };
                self.cursor_moved.write(event.clone());
                self.window_events.write(WindowEvent::CursorMoved(event));
            }
            PointerStep::Button(button, state) => {
                let event = MouseButtonInput {
                    button,
                    state,
                    window,
                };
                self.mouse_buttons.write(event);
                self.window_events
                    .write(WindowEvent::MouseButtonInput(event));
            }
            PointerStep::Wheel(lines) => {
                let event = MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.0,
                    y: lines,
                    window,
                    phase: TouchPhase::Moved,
                };
                self.mouse_wheel.write(event);
                self.window_events.write(WindowEvent::MouseWheel(event));
            }
            PointerStep::Idle => {}
        }
    }

    fn key(&mut self, code: KeyCode, state: ButtonState) {
        let event = KeyboardInput {
            key_code: code,
            logical_key: logical_key(code),
            state,
            text: None,
            repeat: false,
            window: *self.window,
        };
        self.keyboard.write(event.clone());
        self.window_events.write(WindowEvent::KeyboardInput(event));
    }
}

fn drive(
    mut rig: ResMut<Rig>,
    mut input: Input,
    nodes: Query<(&Name, &ComputedNode, &UiGlobalTransform)>,
    screenshots: Query<(), With<Screenshot>>,
    mut app_exit: MessageWriter<AppExit>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
    mut universe: ResMut<Universe>,
    mut view: ResMut<ViewState>,
    mut rng: ResMut<Rng>,
    mut commands: Commands,
) {
    if let Some(code) = rig.key_release.take() {
        input.key(code, ButtonState::Released);
    }
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
    if let Some(step) = rig.pointer.pop_front() {
        input.pointer(step);
        return;
    }

    let Some(action) = rig.actions.pop_front() else {
        return;
    };
    info!("rig: {action:?}");
    // Centre of a named UI node in logical window coordinates.
    let locate = |name: &str| {
        let found = nodes
            .iter()
            .find(|(n, ..)| n.as_str() == name)
            .map(|(_, computed, transform)| transform.translation * computed.inverse_scale_factor);
        if found.is_none() {
            error!("rig: no UI node named {name:?}");
        }
        found
    };
    use ButtonState::{Pressed, Released};
    match action {
        Action::Wait(frames) => rig.wait = frames,
        Action::Shot(name) => {
            let path = rig.dir.join(format!("{name}.png"));
            let entity = commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path))
                .id();
            rig.shot = Some(entity);
        }
        Action::Click { name, offset } => {
            if let Some(center) = locate(&name) {
                rig.pointer.extend([
                    PointerStep::Move(center + offset),
                    PointerStep::Button(MouseButton::Left, Pressed),
                    PointerStep::Button(MouseButton::Left, Released),
                ]);
            }
        }
        Action::Move { name, offset } => {
            if let Some(center) = locate(&name) {
                rig.pointer.push_back(PointerStep::Move(center + offset));
            }
        }
        Action::Drag {
            name,
            delta,
            button,
        } => {
            if let Some(center) = locate(&name) {
                rig.pointer.push_back(PointerStep::Move(center));
                rig.pointer.push_back(PointerStep::Button(button, Pressed));
                for i in 1..=DRAG_STEPS {
                    let along = delta * (i as f32 / DRAG_STEPS as f32);
                    rig.pointer.push_back(PointerStep::Move(center + along));
                }
                rig.pointer.push_back(PointerStep::Button(button, Released));
            }
        }
        Action::Hold { name, frames } => {
            if let Some(center) = locate(&name) {
                rig.pointer.push_back(PointerStep::Move(center));
                rig.pointer
                    .push_back(PointerStep::Button(MouseButton::Left, Pressed));
                rig.pointer
                    .extend(std::iter::repeat_n(PointerStep::Idle, frames as usize));
                rig.pointer
                    .push_back(PointerStep::Button(MouseButton::Left, Released));
            }
        }
        Action::Scroll { name, lines } => {
            if let Some(center) = locate(&name) {
                rig.pointer
                    .extend([PointerStep::Move(center), PointerStep::Wheel(lines)]);
            }
        }
        Action::Key(code) => {
            input.key(code, Pressed);
            rig.key_release = Some(code);
            rig.wait = 1;
        }
        Action::Paint { x, y, alive } => {
            if x < universe.width && y < universe.height {
                universe.set(x, y, alive);
            } else {
                error!("rig: cell ({x}, {y}) is outside the grid");
            }
        }
        Action::Play => playback.playing = true,
        Action::Pause => playback.playing = false,
        Action::Step(steps) => universe.step_by(steps),
        Action::Rule(kind) => universe.set_rule(kind),
        Action::Speed(speed) => {
            playback.speed = speed.clamp(Playback::MIN_SPEED, Playback::MAX_SPEED);
        }
        Action::Stride(stride) => playback.stride = stride.clamp(1, Playback::MAX_STRIDE),
        Action::Vacuum(on) => settings.hide_vacuum = on,
        Action::Reverse(on) => playback.reverse = on,
        Action::Soup(density) => {
            let density = density.unwrap_or(settings.density);
            universe.randomize(density, &mut rng);
        }
        Action::Blob(density) => {
            let density = density.unwrap_or(settings.density);
            universe.randomize_blob(density, &mut rng);
        }
        Action::Clear => universe.clear(),
        Action::Fit => view.fit = true,
        Action::ExpectGeneration(expected) => {
            if universe.generation != expected {
                error!(
                    "rig: expected generation {expected}, found {}",
                    universe.generation
                );
                app_exit.write(AppExit::error());
            }
        }
        Action::ExpectCell { x, y, alive } => {
            let found = x < universe.width && y < universe.height && universe.get(x, y);
            if found != alive {
                error!("rig: expected cell ({x}, {y}) to be {alive}, found {found}");
                app_exit.write(AppExit::error());
            }
        }
        Action::Quit => {
            app_exit.write(AppExit::Success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_script() {
        let actions = parse_script(
            "wait 30; shot start\n click PlayPause; key Space; key [; step -3; rule critters; soup; soup 0.5; paint 3 4 off; quit\n\
             click Speed -40 0; drag Grid 30 -20 right; scroll Grid 3; expect_cell 1 2 on; fit",
        )
        .unwrap();
        assert_eq!(
            actions,
            vec![
                Action::Wait(30),
                Action::Shot("start".into()),
                Action::Click {
                    name: "PlayPause".into(),
                    offset: Vec2::ZERO,
                },
                Action::Key(KeyCode::Space),
                Action::Key(KeyCode::BracketLeft),
                Action::Step(-3),
                Action::Rule(RuleKind::Critters),
                Action::Soup(None),
                Action::Soup(Some(0.5)),
                Action::Paint { x: 3, y: 4, alive: false },
                Action::Quit,
                Action::Click {
                    name: "Speed".into(),
                    offset: Vec2::new(-40.0, 0.0),
                },
                Action::Drag {
                    name: "Grid".into(),
                    delta: Vec2::new(30.0, -20.0),
                    button: MouseButton::Right,
                },
                Action::Scroll {
                    name: "Grid".into(),
                    lines: 3.0,
                },
                Action::ExpectCell { x: 1, y: 2, alive: true },
                Action::Fit,
            ]
        );
    }

    #[test]
    fn comments_run_to_the_end_of_the_line() {
        let actions = parse_script("# a comment; with a semicolon\nwait 1 # trailing; comment\nquit").unwrap();
        assert_eq!(actions, vec![Action::Wait(1), Action::Quit]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_script("wait").is_err());
        assert!(parse_script("frobnicate 1").is_err());
        assert!(parse_script("key F13").is_err());
        assert!(parse_script("rule life").is_err());
        assert!(parse_script("click PlayPause 4").is_err());
        assert!(parse_script("drag Grid 1 2 sideways").is_err());
    }
}
