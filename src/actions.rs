//! What the user can ask for, and the keys that ask for it.
//!
//! Buttons, checkboxes and keys all trigger an [`Action`] and one observer carries it out, so
//! every path does the same thing. The key table also labels the controls with their shortcut.

use bevy::{
    ecs::system::SystemParam,
    input::keyboard::{Key, KeyboardInput},
    input_focus::{FocusedInput, InputFocus},
    prelude::*,
    text::EditableText,
    ui::Pressed,
    ui_widgets::{Activate, MenuItem, ValueChange},
    window::PrimaryWindow,
};

use crate::{
    catcher::Catcher,
    editor::RuleEditor,
    sim::{Playback, Rng, Settings, SimSystems, Universe},
    ui::{Aspect, Control},
    view::{ViewState, WHEEL_ZOOM},
};

/// One thing the user can ask for. (`Default` only serves `bsn!`, which builds the [`Does`] of
/// a control from its default.)
#[derive(Event, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    #[default]
    PlayPause,
    /// One frame back: `stride` generations, or a single one while shift is held.
    StepBack,
    StepForward,
    Flip(Toggle),
    Slower,
    Faster,
    ShorterStride,
    LongerStride,
    Fit,
    ZoomIn,
    ZoomOut,
    Soup,
    Blob,
    Clear,
    /// Make the grid this many cells wide and high.
    Resize(usize),
    EditRule,
    /// Show or hide the list of the spaceships caught.
    Spaceships,
}

/// Put on a button or a checkbox: using it triggers the action.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Does(pub Action);

/// The options that are either on or off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Toggle {
    #[default]
    Reverse,
    HideVacuum,
    ShowGrid,
    ShowBlocks,
    OpenBorder,
    Catching,
}

impl Toggle {
    pub fn aspect(self) -> Aspect {
        match self {
            Toggle::Reverse => Aspect::Time,
            Toggle::HideVacuum | Toggle::ShowGrid | Toggle::ShowBlocks => Aspect::View,
            Toggle::OpenBorder => Aspect::World,
            Toggle::Catching => Aspect::Pattern,
        }
    }

    pub fn get(self, playback: &Playback, settings: &Settings, universe: &Universe) -> bool {
        match self {
            Toggle::Reverse => playback.reverse,
            Toggle::HideVacuum => settings.hide_vacuum,
            Toggle::ShowGrid => settings.show_grid,
            Toggle::ShowBlocks => settings.show_blocks,
            Toggle::OpenBorder => universe.open_border,
            Toggle::Catching => universe.catching,
        }
    }
}

/// Keys and what they do. A key is named by the character it types, so shortcuts follow the
/// keyboard layout. The first key listed for an action is the one shown next to its control.
const KEYS: [(&str, Action); 23] = [
    ("space", Action::PlayPause),
    ("←", Action::StepBack),
    ("→", Action::StepForward),
    ("r", Action::Flip(Toggle::Reverse)),
    ("[", Action::Slower),
    ("]", Action::Faster),
    (",", Action::ShorterStride),
    (".", Action::LongerStride),
    ("v", Action::Flip(Toggle::HideVacuum)),
    ("g", Action::Flip(Toggle::ShowGrid)),
    ("p", Action::Flip(Toggle::ShowBlocks)),
    ("f", Action::Fit),
    ("home", Action::Fit),
    ("+", Action::ZoomIn),
    ("=", Action::ZoomIn),
    ("−", Action::ZoomOut),
    ("n", Action::Soup),
    ("b", Action::Blob),
    ("c", Action::Clear),
    ("o", Action::Flip(Toggle::OpenBorder)),
    ("k", Action::Flip(Toggle::Catching)),
    ("s", Action::Spaceships),
    ("e", Action::EditRule),
];

impl Action {
    fn for_key(key: &Key) -> Option<Action> {
        let name = match key {
            // The key types a hyphen; the table shows a proper minus sign.
            Key::Character(c) if c == "-" => "−".to_string(),
            Key::Character(c) => c.to_lowercase(),
            Key::Space => "space".to_string(),
            Key::ArrowLeft => "←".to_string(),
            Key::ArrowRight => "→".to_string(),
            Key::Home => "home".to_string(),
            _ => return None,
        };
        KEYS.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, action)| *action)
    }

    /// The key to show next to the control for this action.
    pub fn key(self) -> &'static str {
        KEYS.iter()
            .find(|(_, action)| *action == self)
            .map_or("", |(key, _)| key)
    }
}

pub struct ActionsPlugin;

impl Plugin for ActionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(perform)
            .add_observer(on_activate)
            .add_observer(on_toggle)
            .add_systems(Startup, listen_for_keys)
            .add_systems(
                Update,
                (repeat_steps.in_set(SimSystems::Input), release_focus),
            );
    }
}

/// Shortcuts go through the focus system: a key bubbles up to the window only if the focused
/// widget, a text field or an open menu, had no use for it.
fn listen_for_keys(window: Single<Entity, With<PrimaryWindow>>, mut commands: Commands) {
    commands.entity(*window).observe(on_key);
}

fn on_key(
    key: On<FocusedInput<KeyboardInput>>,
    modifiers: Res<ButtonInput<Key>>,
    keyboard_owner: KeyboardOwner,
    mut commands: Commands,
) {
    let input = &key.input;
    // Ctrl+C is not "c", and an open menu keeps the letters it does not use.
    if !input.state.is_pressed()
        || input.repeat
        || modifiers.any_pressed([Key::Control, Key::Alt, Key::Super])
        || keyboard_owner.is_some()
    {
        return;
    }
    if let Some(action) = Action::for_key(&input.logical_key) {
        commands.trigger(action);
    }
}

fn on_activate(activate: On<Activate>, controls: Query<&Does>, mut commands: Commands) {
    if let Ok(&Does(action)) = controls.get(activate.entity) {
        commands.trigger(action);
    }
}

/// A checkbox reports the value it wants; since it always shows the current one, that is a flip.
fn on_toggle(toggle: On<ValueChange<bool>>, controls: Query<&Does>, mut commands: Commands) {
    if let Ok(&Does(action)) = controls.get(toggle.source) {
        commands.trigger(action);
    }
}

fn perform(
    action: On<Action>,
    keys: Res<ButtonInput<KeyCode>>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
    mut universe: ResMut<Universe>,
    mut view: ResMut<ViewState>,
    mut rng: ResMut<Rng>,
    mut editor: ResMut<RuleEditor>,
    mut catcher: ResMut<Catcher>,
) {
    match *action {
        Action::PlayPause => playback.playing = !playback.playing,
        Action::StepBack => universe.step_by(-frame(&keys, &playback)),
        Action::StepForward => universe.step_by(frame(&keys, &playback)),
        Action::Flip(Toggle::Reverse) => playback.reverse = !playback.reverse,
        Action::Flip(Toggle::HideVacuum) => settings.hide_vacuum = !settings.hide_vacuum,
        Action::Flip(Toggle::ShowGrid) => settings.show_grid = !settings.show_grid,
        Action::Flip(Toggle::ShowBlocks) => settings.show_blocks = !settings.show_blocks,
        Action::Flip(Toggle::OpenBorder) => universe.open_border = !universe.open_border,
        Action::Flip(Toggle::Catching) => {
            universe.catching = !universe.catching;
            // Catching is asking what will be caught.
            if universe.catching {
                catcher.show();
            }
        }
        Action::Slower => playback.speed = Control::Speed.snap(playback.speed / 2.0),
        Action::Faster => playback.speed = Control::Speed.snap(playback.speed * 2.0),
        Action::ShorterStride => playback.stride = (playback.stride - 1).max(1),
        Action::LongerStride => playback.stride = (playback.stride + 1).min(Playback::MAX_STRIDE),
        Action::Fit => view.fit = true,
        Action::ZoomIn => view.zoom_about(Vec2::ZERO, WHEEL_ZOOM * WHEEL_ZOOM),
        Action::ZoomOut => view.zoom_about(Vec2::ZERO, 1.0 / (WHEEL_ZOOM * WHEEL_ZOOM)),
        Action::Soup => {
            let density = settings.density;
            universe.randomize(density, &mut rng);
        }
        Action::Blob => {
            let density = settings.density;
            universe.randomize_blob(density, &mut rng);
        }
        Action::Clear => universe.clear(),
        Action::Resize(side) => {
            universe.resize(side, side);
            view.fit = true;
        }
        Action::EditRule => editor.toggle(),
        Action::Spaceships => catcher.toggle(),
    }
}

/// How far one step goes: a frame of `stride` generations, or a single one with shift.
fn frame(keys: &ButtonInput<KeyCode>, playback: &Playback) -> i64 {
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        1
    } else {
        playback.stride as i64
    }
}

/// Holding a step button or an arrow key repeats after this delay, at this interval.
const REPEAT_DELAY: f32 = 0.3;
const REPEAT_INTERVAL: f32 = 1.0 / 12.0;

#[derive(Default)]
struct Hold {
    direction: i64,
    elapsed: f32,
    next: f32,
}

/// Keeps stepping while an arrow key or a step button stays down. The first step is the
/// [`Action`] the press itself triggers, so that even the shortest tap counts.
fn repeat_steps(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    held_buttons: Query<&Does, With<Pressed>>,
    playback: Res<Playback>,
    keyboard_owner: KeyboardOwner,
    mut universe: ResMut<Universe>,
    mut hold: Local<Hold>,
) {
    let mut direction: i64 = held_buttons
        .iter()
        .map(|button| match button.0 {
            Action::StepForward => 1,
            Action::StepBack => -1,
            _ => 0,
        })
        .sum();
    if !keyboard_owner.is_some() {
        direction += keys.pressed(KeyCode::ArrowRight) as i64;
        direction -= keys.pressed(KeyCode::ArrowLeft) as i64;
    }
    let direction = direction.signum();
    if direction == 0 || direction != hold.direction {
        *hold = Hold {
            direction,
            elapsed: 0.0,
            next: REPEAT_DELAY,
        };
        return;
    }
    hold.elapsed += time.delta_secs();
    // At most a few repeats per frame, however long the frame took.
    for _ in 0..4 {
        if hold.elapsed < hold.next {
            break;
        }
        hold.next += REPEAT_INTERVAL;
        universe.step_by(direction * frame(&keys, &playback));
    }
}

/// A focused text field or an open menu owns the keyboard: shortcuts and held arrows stay out
/// of its way.
#[derive(SystemParam)]
pub struct KeyboardOwner<'w, 's> {
    focus: Res<'w, InputFocus>,
    owners: Query<'w, 's, (), Or<(With<EditableText>, With<MenuItem>)>>,
}

impl KeyboardOwner<'_, '_> {
    pub fn is_some(&self) -> bool {
        self.focus
            .get()
            .is_some_and(|entity| self.owners.contains(entity))
    }
}

/// Buttons and checkboxes take the focus when clicked, and a focused button would answer
/// Space and Enter itself instead of letting them reach the shortcuts. So they lose the focus
/// again as soon as the click is over. A text field keeps it until Escape or a press elsewhere
/// (which Bevy handles), an open menu until it closes.
fn release_focus(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    fields: Query<(), With<EditableText>>,
    menu_items: Query<(), With<MenuItem>>,
) {
    let Some(focused) = focus.get() else {
        return;
    };
    let release = if fields.contains(focused) {
        keys.just_pressed(KeyCode::Escape)
    } else {
        !menu_items.contains(focused) && !mouse.pressed(MouseButton::Left)
    };
    if release {
        focus.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_find_their_action_and_back() {
        for (i, (key, action)) in KEYS.iter().enumerate() {
            assert!(
                KEYS[..i].iter().all(|(other, _)| other != key),
                "{key} is listed twice"
            );
            let typed = match *key {
                "space" => Key::Space,
                "←" => Key::ArrowLeft,
                "→" => Key::ArrowRight,
                "home" => Key::Home,
                "−" => Key::Character("-".into()),
                character => Key::Character(character.into()),
            };
            assert_eq!(Action::for_key(&typed), Some(*action), "{key}");
        }
        assert_eq!(Action::for_key(&Key::Character("C".into())), Some(Action::Clear));
        assert_eq!(Action::for_key(&Key::Character("x".into())), None);
        assert_eq!(Action::for_key(&Key::Enter), None);
    }

    #[test]
    fn controls_show_the_first_key_of_their_action() {
        assert_eq!(Action::PlayPause.key(), "space");
        assert_eq!(Action::Fit.key(), "f");
        assert_eq!(Action::ZoomIn.key(), "+");
        assert_eq!(Action::Flip(Toggle::ShowBlocks).key(), "p");
    }
}
