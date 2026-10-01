//! The control panel (Bevy UI + feathers, dark theme), keyboard shortcuts and stepping.

use bevy::{
    feathers::{
        FeathersPlugins,
        constants::fonts,
        controls::{ButtonVariant, FeathersButton, FeathersCheckbox, FeathersRadio},
        cursor::EntityCursor,
        dark_theme::create_dark_theme,
        palette,
        theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemedText, UiTheme},
        tokens,
    },
    input_focus::InputFocus,
    picking::hover::Hovered,
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
    ui::{Checked, Pressed},
    ui_widgets::{
        Activate, RadioGroup, Slider, SliderDragState, SliderOrientation, SliderThumb,
        SliderValue, TrackClick, ValueChange, radio_self_update,
    },
    window::SystemCursorIcon,
};

use crate::{
    rules::RuleKind,
    sim::{Playback, Rng, Settings, Universe, advance},
    view::{ViewState, WHEEL_ZOOM, grid_view},
};

pub const PANEL_WIDTH: f32 = 300.0;

const SLIDER_HEIGHT: f32 = 18.0;
const THUMB: f32 = 14.0;
const RAIL: f32 = 4.0;

/// Holding a step button or an arrow key repeats after this delay, at this interval.
const REPEAT_DELAY: f32 = 0.3;
const REPEAT_INTERVAL: f32 = 1.0 / 12.0;

/// Text nodes whose content mirrors the simulation state.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Readout {
    #[default]
    PlayPauseLabel,
    Generation,
    Population,
    RuleBlurb,
}

/// Checkboxes bound to a boolean in [`Playback`] or [`Settings`].
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Toggle {
    #[default]
    Reverse,
    HideVacuum,
    ShowGrid,
    ShowBlocks,
}

impl Toggle {
    fn get(self, playback: &Playback, settings: &Settings) -> bool {
        match self {
            Toggle::Reverse => playback.reverse,
            Toggle::HideVacuum => settings.hide_vacuum,
            Toggle::ShowGrid => settings.show_grid,
            Toggle::ShowBlocks => settings.show_blocks,
        }
    }
}

/// Sliders bound to a number in [`Playback`] or [`Settings`]. All three are logarithmic, because
/// the useful values span orders of magnitude; the slider itself only knows a position in 0..=1.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Control {
    #[default]
    Speed,
    Stride,
    Density,
}

impl Control {
    fn range(self) -> (f32, f32) {
        match self {
            Control::Speed => (Playback::MIN_SPEED, Playback::MAX_SPEED),
            Control::Stride => (1.0, Playback::MAX_STRIDE as f32),
            Control::Density => (Settings::MIN_DENSITY, Settings::MAX_DENSITY),
        }
    }

    fn get(self, playback: &Playback, settings: &Settings) -> f32 {
        match self {
            Control::Speed => playback.speed,
            Control::Stride => playback.stride as f32,
            Control::Density => settings.density,
        }
    }

    /// Slider position (0..=1) of a value.
    pub fn position_of(self, value: f32) -> f32 {
        let (lo, hi) = self.range();
        (value.clamp(lo, hi) / lo).ln() / (hi / lo).ln()
    }

    /// The value at a slider position, rounded to something pleasant.
    pub fn value_at(self, position: f32) -> f32 {
        let (lo, hi) = self.range();
        let value = lo * (hi / lo).powf(position.clamp(0.0, 1.0));
        let rounded = match self {
            Control::Speed if value < 10.0 => (value * 2.0).round() / 2.0,
            Control::Speed | Control::Stride => value.round(),
            Control::Density => {
                // Two significant digits.
                let magnitude = 10f32.powf(value.log10().floor() - 1.0);
                (value / magnitude).round() * magnitude
            }
        };
        rounded.clamp(lo, hi)
    }

    fn format(self, value: f32) -> String {
        match self {
            Control::Speed if value < 10.0 => format!("{value:.1} fps"),
            Control::Speed => format!("{value:.0} fps"),
            Control::Stride => format!("{value:.0}"),
            Control::Density => {
                let percent = 100.0 * value;
                let decimals = match percent {
                    p if p >= 10.0 => 0,
                    p if p >= 1.0 => 1,
                    p if p >= 0.1 => 2,
                    _ => 3,
                };
                format!("{percent:.decimals$} %")
            }
        }
    }
}

/// The text showing a slider's value.
#[derive(Component, Clone, Copy, Debug, Default)]
struct ValueLabel(Control);

/// The filled part of a slider's rail.
#[derive(Component, Clone, Copy, Debug, Default)]
struct SliderFill;

/// The rule a radio button selects.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RuleChoice(pub RuleKind);

/// A button that steps time by one frame while held: `+1` forwards, `-1` backwards.
#[derive(Component, Clone, Copy, Debug, Default)]
struct StepButton(i64);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FeathersPlugins)
            .insert_resource(UiTheme(create_dark_theme()))
            .add_systems(Startup, spawn_ui)
            .add_systems(
                Update,
                (
                    (keyboard_shortcuts, stepping).before(advance),
                    release_focus,
                    (sync_widgets, sync_sliders, style_sliders)
                        .chain()
                        .after(advance),
                    update_status.after(advance),
                ),
            );
    }
}

fn spawn_ui(
    mut commands: Commands,
    universe: Res<Universe>,
    playback: Res<Playback>,
    settings: Res<Settings>,
) {
    commands.spawn(Camera2d);
    let position = |control: Control| control.position_of(control.get(&playback, &settings));
    commands.spawn_scene(root(
        universe.kind(),
        position(Control::Speed),
        position(Control::Stride),
        position(Control::Density),
    ));
}

fn root(rule: RuleKind, speed: f32, stride: f32, density: f32) -> impl Scene {
    bsn! {
        #Root
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Row,
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            panel(rule, speed, stride, density),
            grid_view(),
        ]
    }
}

fn panel(rule: RuleKind, speed: f32, stride: f32, density: f32) -> impl Scene {
    bsn! {
        #Panel
        Node {
            width: px(PANEL_WIDTH),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            padding: px(16),
            row_gap: px(14),
            border: UiRect { right: px(1) },
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        ThemeBorderColor(tokens::PANE_HEADER_BORDER)
        Children [
            header(),
            rule_section(rule),
            time_section(),
            speed_section(speed, stride),
            view_section(),
            world_section(density),
            (Node { flex_grow: 1.0 }),
            help(),
        ]
    }
}

fn header() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
        }
        Children [
            (
                Text("cas")
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(24.0),
                    weight: FontWeight::BOLD,
                }
                TextColor(palette::WHITE)
            ),
            caption("reversible block cellular automata"),
        ]
    }
}

/// A titled group of controls.
fn section(title: &'static str, body: impl SceneList) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(6),
        }
        Children [
            (
                Text(title)
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(11.0),
                    weight: FontWeight::BOLD,
                }
                ThemeTextColor(tokens::TEXT_DIM)
            ),
            { body },
        ]
    }
}

/// Small dim explanatory text.
fn caption(text: impl Into<String>) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(12.0),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT_DIM)
    }
}

/// Monospace readout text.
fn readout(text: impl Into<String>) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::MONO),
            font_size: FontSize::Px(12.0),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT_MAIN)
    }
}

/// A checkbox bound to `toggle`. Its checked state always follows the resource, see
/// [`sync_widgets`].
fn toggle(label: &'static str, name: &'static str, toggle: Toggle) -> impl Scene {
    let name = Name::new(name);
    bsn! {
        @FeathersCheckbox {
            @caption: bsn! { Text(label) ThemedText }
        }
        template_value(name)
        template_value(toggle)
        on(toggle_changed)
    }
}

/// A caption, the current value, and a slider underneath.
fn slider_row(
    label: &'static str,
    name: &'static str,
    control: Control,
    position: f32,
) -> impl Scene {
    let value_label = ValueLabel(control);
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
        }
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                }
                Children [
                    caption(label),
                    (readout("") template_value(value_label)),
                ]
            ),
            slider(name, control, position),
        ]
    }
}

/// A slider with a rail, a fill and a thumb, on top of the headless `Slider` widget: clicking
/// the rail jumps there, dragging follows the pointer. The thumb travels inside a box that is
/// one thumb narrower than the slider, so plain percentages place it.
fn slider(name: &'static str, control: Control, position: f32) -> impl Scene {
    let name = Name::new(name);
    bsn! {
        Node {
            height: px(SLIDER_HEIGHT),
            align_items: AlignItems::Center,
        }
        template_value(name)
        template_value(control)
        Slider {
            track_click: TrackClick::Snap,
            orientation: SliderOrientation::Horizontal,
        }
        SliderValue(position)
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        on(slider_changed)
        Children [
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    height: px(RAIL),
                    border_radius: BorderRadius::MAX,
                }
                BackgroundColor(palette::GRAY_3)
            ),
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(THUMB),
                    top: px(0),
                    bottom: px(0),
                    align_items: AlignItems::Center,
                }
                Children [
                    (
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            width: percent(0),
                            height: px(RAIL),
                            border_radius: BorderRadius::MAX,
                        }
                        BackgroundColor(palette::ACCENT)
                        SliderFill
                    ),
                    (
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(0),
                            width: px(THUMB),
                            height: px(THUMB),
                            border_radius: BorderRadius::MAX,
                        }
                        BackgroundColor(palette::LIGHT_GRAY_1)
                        SliderThumb
                    ),
                ]
            ),
        ]
    }
}

fn rule_section(rule: RuleKind) -> impl Scene {
    let blurb = rule.blurb();
    section("RULE", bsn_list![
        (
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
            }
            RadioGroup
            on(radio_self_update)
            on(|change: On<ValueChange<Entity>>,
                choices: Query<&RuleChoice>,
                mut universe: ResMut<Universe>| {
                if let Ok(choice) = choices.get(change.value) {
                    universe.set_rule(choice.0);
                }
            })
            Children [
                rule_radio(RuleKind::SingleRotation),
                rule_radio(RuleKind::Critters),
            ]
        ),
        (caption(blurb) template_value(Readout::RuleBlurb)),
    ])
}

fn rule_radio(kind: RuleKind) -> impl Scene {
    // BSN values are literals, variables or `{ expressions }`; method calls go in variables.
    let label = kind.name();
    let name = Name::new(kind.id());
    let choice = RuleChoice(kind);
    bsn! {
        @FeathersRadio {
            @caption: bsn! { Text(label) ThemedText }
        }
        template_value(name)
        template_value(choice)
    }
}

fn time_section() -> impl Scene {
    let back = StepButton(-1);
    let forward = StepButton(1);
    section("TIME", bsn_list![
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            Children [
                (
                    // Stepping is driven by `stepping`, which also repeats while held.
                    #StepBack
                    @FeathersButton {
                        @caption: bsn! { Text("← step") ThemedText }
                    }
                    Node { flex_grow: 1.0 }
                    template_value(back)
                ),
                (
                    #PlayPause
                    @FeathersButton {
                        @caption: bsn! { Text("Play") ThemedText template_value(Readout::PlayPauseLabel) },
                        @variant: ButtonVariant::Primary,
                    }
                    Node {
                        flex_grow: 1.5,
                        min_width: px(84),
                    }
                    on(|_: On<Activate>, mut playback: ResMut<Playback>| {
                        playback.playing = !playback.playing;
                    })
                ),
                (
                    #StepForward
                    @FeathersButton {
                        @caption: bsn! { Text("step →") ThemedText }
                    }
                    Node { flex_grow: 1.0 }
                    template_value(forward)
                ),
            ]
        ),
        toggle("Run backwards in time", "Reverse", Toggle::Reverse),
        (
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
            }
            Children [
                (readout("generation 0") template_value(Readout::Generation)),
                (readout("population 0") template_value(Readout::Population)),
            ]
        ),
    ])
}

fn speed_section(speed: f32, stride: f32) -> impl Scene {
    section("SPEED", bsn_list![
        slider_row("Frames per second", "Speed", Control::Speed, speed),
        slider_row("Generations per frame", "Stride", Control::Stride, stride),
    ])
}

fn view_section() -> impl Scene {
    section("VIEW", bsn_list![
        toggle("Hide vacuum fluctuations", "HideVacuum", Toggle::HideVacuum),
        toggle("Cell grid", "ShowGrid", Toggle::ShowGrid),
        toggle("2×2 blocks a forward step rotates", "ShowBlocks", Toggle::ShowBlocks),
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
            }
            Children [
                (
                    #FitView
                    @FeathersButton {
                        @caption: bsn! { Text("Fit") ThemedText }
                    }
                    Node { min_width: px(56) }
                    on(|_: On<Activate>, mut view: ResMut<ViewState>| {
                        view.fit = true;
                    })
                ),
                caption("wheel zooms · right-drag pans"),
            ]
        ),
    ])
}

fn world_section(density: f32) -> impl Scene {
    section("WORLD", bsn_list![
        slider_row("Density", "Density", Control::Density, density),
        (
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6),
            }
            Children [
                (
                    #Soup
                    @FeathersButton {
                        @caption: bsn! { Text("Soup") ThemedText }
                    }
                    Node { flex_grow: 1.0 }
                    on(|_: On<Activate>,
                        settings: Res<Settings>,
                        mut rng: ResMut<Rng>,
                        mut universe: ResMut<Universe>| {
                        let density = settings.density;
                        universe.randomize(density, &mut rng);
                    })
                ),
                (
                    #Blob
                    @FeathersButton {
                        @caption: bsn! { Text("Blob") ThemedText }
                    }
                    Node { flex_grow: 1.0 }
                    on(|_: On<Activate>,
                        settings: Res<Settings>,
                        mut rng: ResMut<Rng>,
                        mut universe: ResMut<Universe>| {
                        let density = settings.density;
                        universe.randomize_blob(density, &mut rng);
                    })
                ),
                (
                    #Clear
                    @FeathersButton {
                        @caption: bsn! { Text("Clear") ThemedText }
                    }
                    Node { flex_grow: 1.0 }
                    on(|_: On<Activate>, mut universe: ResMut<Universe>| {
                        universe.clear();
                    })
                ),
            ]
        ),
        caption("Soup fills the grid, blob seeds a square in the middle. Left-drag paints, with shift it erases."),
    ])
}

fn help() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
        }
        Children [
            caption("space play/pause · ← → step (shift: one)"),
            caption("r reverse · v vacuum · g grid · p blocks"),
            caption("f fit · + − zoom · [ ] speed · , . stride"),
            caption("n soup · b blob · c clear"),
        ]
    }
}

fn slider_changed(
    change: On<ValueChange<f32>>,
    controls: Query<&Control>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let value = control.value_at(change.value);
    // Only write real changes: writing marks the resource changed and re-syncs the panel.
    match control {
        Control::Speed if playback.speed != value => playback.speed = value,
        Control::Stride if playback.stride != value as u32 => playback.stride = value as u32,
        Control::Density if settings.density != value => settings.density = value,
        _ => {}
    }
}

fn toggle_changed(
    change: On<ValueChange<bool>>,
    toggles: Query<&Toggle>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
) {
    let Ok(toggle) = toggles.get(change.source) else {
        return;
    };
    match toggle {
        Toggle::Reverse => playback.reverse = change.value,
        Toggle::HideVacuum => settings.hide_vacuum = change.value,
        Toggle::ShowGrid => settings.show_grid = change.value,
        Toggle::ShowBlocks => settings.show_blocks = change.value,
    }
}

fn keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
    mut universe: ResMut<Universe>,
    mut view: ResMut<ViewState>,
    mut rng: ResMut<Rng>,
) {
    let pressed = |codes: &[KeyCode]| codes.iter().any(|code| keys.just_pressed(*code));
    if pressed(&[KeyCode::Space]) {
        playback.playing = !playback.playing;
    }
    if pressed(&[KeyCode::KeyR]) {
        playback.reverse = !playback.reverse;
    }
    if pressed(&[KeyCode::KeyV]) {
        settings.hide_vacuum = !settings.hide_vacuum;
    }
    if pressed(&[KeyCode::KeyG]) {
        settings.show_grid = !settings.show_grid;
    }
    if pressed(&[KeyCode::KeyP]) {
        settings.show_blocks = !settings.show_blocks;
    }
    if pressed(&[KeyCode::KeyF, KeyCode::Home]) {
        view.fit = true;
    }
    if pressed(&[KeyCode::Equal, KeyCode::NumpadAdd]) {
        view.zoom_about(Vec2::ZERO, WHEEL_ZOOM * WHEEL_ZOOM);
    }
    if pressed(&[KeyCode::Minus, KeyCode::NumpadSubtract]) {
        view.zoom_about(Vec2::ZERO, 1.0 / (WHEEL_ZOOM * WHEEL_ZOOM));
    }
    if pressed(&[KeyCode::KeyN]) {
        let density = settings.density;
        universe.randomize(density, &mut rng);
    }
    if pressed(&[KeyCode::KeyB]) {
        let density = settings.density;
        universe.randomize_blob(density, &mut rng);
    }
    if pressed(&[KeyCode::KeyC]) {
        universe.clear();
    }
    if pressed(&[KeyCode::BracketLeft]) {
        playback.speed = (playback.speed / 2.0).max(Playback::MIN_SPEED);
    }
    if pressed(&[KeyCode::BracketRight]) {
        playback.speed = (playback.speed * 2.0).min(Playback::MAX_SPEED);
    }
    if pressed(&[KeyCode::Comma]) {
        playback.stride = (playback.stride - 1).max(1);
    }
    if pressed(&[KeyCode::Period]) {
        playback.stride = (playback.stride + 1).min(Playback::MAX_STRIDE);
    }
}

#[derive(Default)]
struct Hold {
    direction: i64,
    elapsed: f32,
    next: f32,
}

/// Steps time while an arrow key or a step button is held: once immediately, then repeating
/// after a short delay. A frame is `stride` generations, or a single one with shift.
fn stepping(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<&StepButton, With<Pressed>>,
    playback: Res<Playback>,
    mut universe: ResMut<Universe>,
    mut hold: Local<Hold>,
) {
    let mut direction = buttons.iter().map(|button| button.0).sum::<i64>();
    if keys.pressed(KeyCode::ArrowRight) {
        direction += 1;
    }
    if keys.pressed(KeyCode::ArrowLeft) {
        direction -= 1;
    }
    let direction = direction.signum();
    if direction == 0 {
        *hold = Hold::default();
        return;
    }
    let single = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let steps = direction * if single { 1 } else { playback.stride as i64 };
    if hold.direction != direction {
        *hold = Hold {
            direction,
            elapsed: 0.0,
            next: REPEAT_DELAY,
        };
        universe.step_by(steps);
        return;
    }
    hold.elapsed += time.delta_secs();
    // At most a few repeats per frame, however long the frame took.
    for _ in 0..4 {
        if hold.elapsed < hold.next {
            break;
        }
        hold.next += REPEAT_INTERVAL;
        universe.step_by(steps);
    }
}

/// Clicking a widget gives it keyboard focus, which would make Space/arrows drive the widget
/// instead of the simulation. We don't need focus-driven widgets, so drop it after every click.
fn release_focus(mouse: Res<ButtonInput<MouseButton>>, mut focus: ResMut<InputFocus>) {
    if mouse.just_released(MouseButton::Left) && focus.get().is_some() {
        focus.clear();
    }
}

/// Pushes resource state into the radios, checkboxes and labels, so changes from anywhere
/// (the widgets themselves, keyboard shortcuts, the test rig) show up in the panel. Also runs
/// when the widgets appear, since the scene is spawned asynchronously.
fn sync_widgets(
    universe: Res<Universe>,
    playback: Res<Playback>,
    settings: Res<Settings>,
    radios: Query<(Entity, &RuleChoice, Has<Checked>)>,
    toggles: Query<(Entity, &Toggle, Has<Checked>)>,
    mut readouts: Query<(&Readout, &mut Text)>,
    added: Query<(), Or<(Added<RuleChoice>, Added<Toggle>, Added<Readout>)>>,
    mut last_rule: Local<Option<RuleKind>>,
    mut commands: Commands,
) {
    let fresh = !added.is_empty();
    let kind = universe.kind();
    let rule_changed = *last_rule != Some(kind);
    *last_rule = Some(kind);
    if !(fresh || rule_changed || playback.is_changed() || settings.is_changed()) {
        return;
    }
    for (entity, choice, checked) in &radios {
        set_checked(&mut commands, entity, checked, choice.0 == kind);
    }
    for (entity, toggle, checked) in &toggles {
        set_checked(&mut commands, entity, checked, toggle.get(&playback, &settings));
    }
    for (readout, mut text) in &mut readouts {
        let content = match readout {
            Readout::PlayPauseLabel if playback.playing => "Pause",
            Readout::PlayPauseLabel => "Play",
            Readout::RuleBlurb => kind.blurb(),
            Readout::Generation | Readout::Population => continue,
        };
        if text.0 != content {
            text.0 = content.into();
        }
    }
}

fn set_checked(commands: &mut Commands, entity: Entity, is_checked: bool, checked: bool) {
    if checked && !is_checked {
        commands.entity(entity).insert(Checked);
    } else if !checked && is_checked {
        commands.entity(entity).remove::<Checked>();
    }
}

/// Moves the sliders and their value labels to where the resources say they are.
fn sync_sliders(
    playback: Res<Playback>,
    settings: Res<Settings>,
    sliders: Query<(Entity, &Control, &SliderValue)>,
    mut labels: Query<(&ValueLabel, &mut Text)>,
    added: Query<(), Or<(Added<Control>, Added<ValueLabel>)>>,
    mut commands: Commands,
) {
    if !(playback.is_changed() || settings.is_changed() || !added.is_empty()) {
        return;
    }
    for (entity, control, current) in &sliders {
        let position = control.position_of(control.get(&playback, &settings));
        if (current.0 - position).abs() > 1e-6 {
            commands.entity(entity).insert(SliderValue(position));
        }
    }
    for (label, mut text) in &mut labels {
        let content = label.0.format(label.0.get(&playback, &settings));
        if text.0 != content {
            text.0 = content;
        }
    }
}

/// Places each slider's thumb and fill, and highlights the thumb while hovered or dragged.
fn style_sliders(
    sliders: Query<
        (Entity, &SliderValue, &Hovered, &SliderDragState),
        (
            With<Control>,
            Or<(
                Changed<SliderValue>,
                Changed<Hovered>,
                Changed<SliderDragState>,
            )>,
        ),
    >,
    children: Query<&Children>,
    mut thumbs: Query<(&mut Node, &mut BackgroundColor), (With<SliderThumb>, Without<SliderFill>)>,
    mut fills: Query<&mut Node, (With<SliderFill>, Without<SliderThumb>)>,
) {
    for (slider, value, hovered, drag) in &sliders {
        let position = percent(100.0 * value.0.clamp(0.0, 1.0));
        let color = if hovered.0 || drag.dragging {
            palette::WHITE
        } else {
            palette::LIGHT_GRAY_1
        };
        for child in children.iter_descendants(slider) {
            if let Ok((mut node, mut background)) = thumbs.get_mut(child) {
                node.left = position;
                background.0 = color;
            }
            if let Ok(mut node) = fills.get_mut(child) {
                node.width = position;
            }
        }
    }
}

fn update_status(
    universe: Res<Universe>,
    playback: Res<Playback>,
    mut readouts: Query<(&Readout, &mut Text)>,
    added: Query<(), Added<Readout>>,
) {
    if !(universe.is_changed() || playback.is_changed() || !added.is_empty()) {
        return;
    }
    let state = if playback.playing {
        let rate = playback.speed * playback.stride as f32;
        let digits = if rate < 10.0 { 1 } else { 0 };
        format!(
            "running {} · {rate:.digits$} gen/s",
            if playback.reverse { "backwards" } else { "forwards" },
        )
    } else {
        "paused".to_string()
    };
    let population = universe.population();
    let cells = universe.width * universe.height;
    for (readout, mut text) in &mut readouts {
        match readout {
            Readout::Generation => {
                text.0 = format!("generation {}  ·  {state}", group_digits(universe.generation));
            }
            Readout::Population => {
                text.0 = format!(
                    "population {} ({:.2}%)",
                    group_digits(population as i64),
                    100.0 * population as f64 / cells as f64,
                );
            }
            _ => {}
        }
    }
}

/// `1234567` → `1 234 567`.
fn group_digits(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_positions_round_trip() {
        for (control, value) in [
            (Control::Speed, 30.0),
            (Control::Speed, 0.5),
            (Control::Speed, 240.0),
            (Control::Stride, 1.0),
            (Control::Stride, 7.0),
            (Control::Stride, 512.0),
            (Control::Density, 0.3),
            (Control::Density, 0.0001),
            (Control::Density, 0.0025),
        ] {
            let back = control.value_at(control.position_of(value));
            assert!(
                (back - value).abs() <= 1e-3 * value,
                "{control:?}: {value} came back as {back}"
            );
        }
    }

    #[test]
    fn slider_ends_are_the_range() {
        for control in [Control::Speed, Control::Stride, Control::Density] {
            let (lo, hi) = control.range();
            assert_eq!(control.value_at(0.0), lo);
            assert_eq!(control.value_at(1.0), hi);
            assert_eq!(control.position_of(lo), 0.0);
            assert!((control.position_of(hi) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn values_are_formatted_for_humans() {
        assert_eq!(Control::Speed.format(30.0), "30 fps");
        assert_eq!(Control::Speed.format(0.5), "0.5 fps");
        assert_eq!(Control::Stride.format(16.0), "16");
        assert_eq!(Control::Density.format(0.3), "30 %");
        assert_eq!(Control::Density.format(0.025), "2.5 %");
        assert_eq!(Control::Density.format(0.001), "0.10 %");
        assert_eq!(Control::Density.format(0.0001), "0.010 %");
        assert_eq!(group_digits(-1234567), "-1 234 567");
    }
}
