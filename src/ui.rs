//! The control panel (Bevy UI + feathers, dark theme) and the systems that keep it showing
//! the state of the simulation.

use bevy::{
    feathers::{
        FeathersPlugins,
        constants::fonts,
        controls::{
            ButtonVariant, FeathersButton, FeathersCheckbox, FeathersMenu, FeathersMenuButton,
            FeathersMenuDivider, FeathersMenuItem, FeathersMenuPopup,
        },
        cursor::EntityCursor,
        dark_theme::create_dark_theme,
        palette,
        theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemedText, UiTheme},
        tokens,
    },
    picking::hover::Hovered,
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
    ui::Checked,
    ui_widgets::{
        Activate, ActivateOnPress, Slider, SliderDragState, SliderOrientation, SliderThumb,
        SliderValue, TrackClick, ValueChange,
    },
    window::SystemCursorIcon,
};

use crate::{
    actions::{Action, Does, Toggle},
    catcher::catcher_panel,
    editor::{RuleEditor, describe, editor_panel},
    rules::PRESETS,
    sim::{Pace, Playback, Settings, SimSystems, Universe, rule_changed},
    view::{grid_view, wheel_notches},
};

pub const PANEL_WIDTH: f32 = 300.0;

/// The grid sizes on offer: so many cells each way.
const GRID_SIDES: [usize; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];

const SLIDER_HEIGHT: f32 = 18.0;
const THUMB: f32 = 14.0;
const RAIL: f32 = 4.0;

/// Shortcut reminders are legible on a button of any colour, and quiet.
const KEY_HINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.45);

/// Text nodes whose content mirrors the simulation state.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Readout {
    #[default]
    PlayPauseLabel,
    Generation,
    Transport,
    Population,
    RuleName,
    RuleBlurb,
    GridSize,
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

    /// The nearest value the slider can show.
    pub fn snap(self, value: f32) -> f32 {
        self.value_at(self.position_of(value))
    }

    /// The next value up or down: a 64th of the slider, or as much further as it takes for
    /// rounding to land on a value beyond the current one.
    pub fn nudged(self, value: f32, up: bool) -> f32 {
        let step = if up { 1.0 / 64.0 } else { -1.0 / 64.0 };
        let mut position = self.position_of(value);
        loop {
            position = (position + step).clamp(0.0, 1.0);
            let next = self.value_at(position);
            let moved = if up { next > value } else { next < value };
            if moved || position <= 0.0 || position >= 1.0 {
                return next;
            }
        }
    }

    /// Writes a value to its resource, but only if it differs: writing marks the resource
    /// changed and re-syncs the panel.
    fn set(self, value: f32, playback: &mut ResMut<Playback>, settings: &mut ResMut<Settings>) {
        match self {
            Control::Speed if playback.speed != value => playback.speed = value,
            Control::Stride if playback.stride != value as u32 => playback.stride = value as u32,
            Control::Density if settings.density != value => settings.density = value,
            _ => {}
        }
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

/// The preset (an index into [`PRESETS`]) a rule-menu item selects.
#[derive(Component, Clone, Copy, Debug, Default)]
struct RuleChoice(usize);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FeathersPlugins)
            .insert_resource(UiTheme(create_dark_theme()))
            .add_systems(Startup, spawn_ui)
            .add_systems(
                Update,
                (
                    sync_widgets.run_if(rule_changed.or_eager(options_changed)),
                    (sync_sliders.run_if(options_changed), style_sliders).chain(),
                    update_status,
                )
                    .in_set(SimSystems::Present),
            );
    }
}

/// Has anything a checkbox, a slider or the size menu shows changed?
fn options_changed(
    playback: Res<Playback>,
    settings: Res<Settings>,
    universe: Res<Universe>,
    mut seen: Local<Option<(bool, bool, usize, usize)>>,
) -> bool {
    // The universe counts as changed on every step; its options are watched by value.
    let world = (universe.open_border, universe.catching, universe.width, universe.height);
    playback.is_changed() || settings.is_changed() || seen.replace(world) != Some(world)
}

fn spawn_ui(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn_scene(root());
}

fn root() -> impl Scene {
    bsn! {
        #Root
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Row,
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            panel(),
            editor_panel(),
            catcher_panel(),
            grid_view(),
        ]
    }
}

fn panel() -> impl Scene {
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
            rule_section(),
            world_section(),
            time_section(),
            speed_section(),
            view_section(),
            pattern_section(),
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
pub(crate) fn section(title: &'static str, body: impl SceneList) -> impl Scene {
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
pub(crate) fn caption(text: impl Into<String>) -> impl Scene {
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
pub(crate) fn readout(text: impl Into<String>) -> impl Scene {
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

/// A quiet reminder of a shortcut, placed right after the label of its control.
fn key_hint(keys: impl Into<String>) -> impl Scene {
    bsn! {
        Text(keys)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::MONO),
            font_size: FontSize::Px(10.0),
            weight: FontWeight::NORMAL,
        }
        TextColor(KEY_HINT)
        Node {
            margin: UiRect { left: px(6) },
        }
    }
}

/// The caption of a button or checkbox: its label and the key that does the same.
fn label_with_key(label: &'static str, action: Action) -> Box<dyn SceneList> {
    bsn_list![
        (Text(label) ThemedText),
        key_hint(action.key()),
    ]
    .into()
}

/// A button that triggers `action`, labelled with its shortcut.
fn action_button(label: &'static str, name: &'static str, action: Action) -> impl Scene {
    let name = Name::new(name);
    let does = Does(action);
    bsn! {
        @FeathersButton {
            @caption: {label_with_key(label, action)},
        }
        Node { flex_grow: 1.0 }
        template_value(name)
        template_value(does)
    }
}

/// A checkbox for an on/off option. Its checked state always follows the resource, see
/// [`sync_widgets`].
pub(crate) fn toggle(label: &'static str, name: &'static str, option: Toggle) -> impl Scene {
    let name = Name::new(name);
    let does = Does(Action::Flip(option));
    bsn! {
        @FeathersCheckbox {
            @caption: {label_with_key(label, Action::Flip(option))},
        }
        template_value(name)
        template_value(does)
    }
}

/// A caption with the keys that nudge the value, the current value, and a slider underneath.
fn slider_row(label: &'static str, name: &'static str, control: Control, keys: String) -> impl Scene {
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
                    align_items: AlignItems::Center,
                }
                Children [
                    caption(label),
                    key_hint(keys),
                    (Node { flex_grow: 1.0 }),
                    (readout("") template_value(value_label)),
                ]
            ),
            slider(name, control),
        ]
    }
}

/// A slider with a rail, a fill and a thumb, on top of the headless `Slider` widget: clicking
/// the rail jumps there, dragging follows the pointer, the wheel steps. The thumb travels
/// inside a box that is one thumb narrower than the slider, so plain percentages place it.
fn slider(name: &'static str, control: Control) -> impl Scene {
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
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        on(slider_changed)
        on(slider_scrolled)
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

/// The rule menu, a button for the editor, and what the current rule does.
fn rule_section() -> impl Scene {
    let presets: Vec<_> = (0..PRESETS.len()).map(rule_item).collect();
    section("RULE", bsn_list![
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            Children [
                (
                    @FeathersMenu
                    Node { flex_grow: 1.0 }
                    Children [
                        (
                            #RuleMenu
                            @FeathersMenuButton {
                                @caption: bsn! { Text("") ThemedText template_value(Readout::RuleName) }
                            }
                            Node { flex_grow: 1.0 }
                        ),
                        (
                            @FeathersMenuPopup
                            Children [
                                { presets },
                                @FeathersMenuDivider,
                                (
                                    #RuleItemCustom
                                    @FeathersMenuItem {
                                        @caption: bsn! { Text("Custom…") ThemedText }
                                    }
                                    on(|_: On<Activate>,
                                        mut editor: ResMut<RuleEditor>,
                                        mut universe: ResMut<Universe>| {
                                        editor.open_custom(&mut universe);
                                    })
                                ),
                            ]
                        ),
                    ]
                ),
                (
                    action_button("Edit", "EditRule", Action::EditRule)
                    Node { flex_grow: 0.0 }
                ),
            ]
        ),
        (caption("") template_value(Readout::RuleBlurb)),
    ])
}

fn rule_item(index: usize) -> impl Scene {
    let preset = &PRESETS[index];
    let label = preset.name;
    let name = Name::new(format!("RuleItem:{}", preset.id));
    let choice = RuleChoice(index);
    bsn! {
        @FeathersMenuItem {
            @caption: bsn! { Text(label) ThemedText }
        }
        template_value(name)
        template_value(choice)
        on(|activate: On<Activate>, choices: Query<&RuleChoice>, mut universe: ResMut<Universe>| {
            if let Ok(choice) = choices.get(activate.entity) {
                universe.set_rule(PRESETS[choice.0].rule());
            }
        })
    }
}

/// The space the automaton lives in: how big it is, and what its edge does.
fn world_section() -> impl Scene {
    let sizes: Vec<_> = GRID_SIDES.into_iter().map(size_item).collect();
    section("WORLD", bsn_list![
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
            }
            Children [
                caption("Grid size"),
                (Node { flex_grow: 1.0 }),
                (
                    @FeathersMenu
                    Children [
                        (
                            #GridSize
                            @FeathersMenuButton {
                                @caption: bsn! { Text("") ThemedText template_value(Readout::GridSize) }
                            }
                        ),
                        (
                            @FeathersMenuPopup
                            Children [ { sizes } ]
                        ),
                    ]
                ),
            ]
        ),
        toggle("Open border", "OpenBorder", Toggle::OpenBorder),
    ])
}

/// A size in the grid-size menu.
fn size_item(side: usize) -> impl Scene {
    let label = format!("{side} × {side}");
    let name = Name::new(format!("GridSize:{side}"));
    let does = Does(Action::Resize(side));
    bsn! {
        @FeathersMenuItem {
            @caption: bsn! { Text(label) ThemedText }
        }
        template_value(name)
        template_value(does)
    }
}

fn time_section() -> impl Scene {
    // The step buttons act on the press, so that holding them can repeat (`repeat_steps`).
    let back = Does(Action::StepBack);
    let forward = Does(Action::StepForward);
    let play = Does(Action::PlayPause);
    let play_caption: Box<dyn SceneList> = bsn_list![
        (Text("Play") ThemedText template_value(Readout::PlayPauseLabel)),
        key_hint(Action::PlayPause.key()),
    ]
    .into();
    section("TIME", bsn_list![
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            Children [
                (
                    #StepBack
                    @FeathersButton {
                        @caption: bsn! { Text("← step") ThemedText }
                    }
                    Node { flex_grow: 1.0 }
                    ActivateOnPress
                    template_value(back)
                ),
                (
                    #PlayPause
                    @FeathersButton {
                        @caption: {play_caption},
                        @variant: ButtonVariant::Primary,
                    }
                    Node {
                        flex_grow: 1.5,
                        min_width: px(96),
                    }
                    template_value(play)
                ),
                (
                    #StepForward
                    @FeathersButton {
                        @caption: bsn! { Text("step →") ThemedText }
                    }
                    Node { flex_grow: 1.0 }
                    ActivateOnPress
                    template_value(forward)
                ),
            ]
        ),
        caption("hold to repeat · with shift: a single generation"),
        toggle("Run backwards in time", "Reverse", Toggle::Reverse),
        (
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
            }
            Children [
                (readout("") template_value(Readout::Generation)),
                (readout("") template_value(Readout::Transport)),
                (readout("") template_value(Readout::Population)),
            ]
        ),
    ])
}

fn speed_section() -> impl Scene {
    let keys = |down: Action, up: Action| format!("{} {}", down.key(), up.key());
    section("SPEED", bsn_list![
        slider_row(
            "Frames per second",
            "Speed",
            Control::Speed,
            keys(Action::Slower, Action::Faster),
        ),
        slider_row(
            "Generations per frame",
            "Stride",
            Control::Stride,
            keys(Action::ShorterStride, Action::LongerStride),
        ),
    ])
}

fn view_section() -> impl Scene {
    let navigation = format!(
        "wheel or {} {} zooms · right-drag pans",
        Action::ZoomIn.key(),
        Action::ZoomOut.key(),
    );
    section("VIEW", bsn_list![
        toggle("Hide vacuum fluctuations", "HideVacuum", Toggle::HideVacuum),
        toggle("Cell grid", "ShowGrid", Toggle::ShowGrid),
        toggle("2×2 blocks of the next step", "ShowBlocks", Toggle::ShowBlocks),
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
            }
            Children [
                (
                    action_button("Fit", "FitView", Action::Fit)
                    Node { flex_grow: 0.0 }
                ),
                caption(navigation),
            ]
        ),
    ])
}

/// What is in the world: seeding it, and catching the spaceships that reach its edge.
fn pattern_section() -> impl Scene {
    section("PATTERN", bsn_list![
        slider_row("Density", "Density", Control::Density, String::new()),
        (
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6),
            }
            Children [
                action_button("Soup", "Soup", Action::Soup),
                action_button("Blob", "Blob", Action::Blob),
                action_button("Clear", "Clear", Action::Clear),
            ]
        ),
        caption("Soup fills the grid, blob seeds a square in the middle. Left-drag paints, with shift it erases."),
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
            }
            Children [
                (
                    toggle("Catch spaceships", "Catching", Toggle::Catching)
                    Node { flex_grow: 1.0 }
                ),
                (
                    action_button("Spaceships", "Spaceships", Action::Spaceships)
                    Node { flex_grow: 0.0 }
                ),
            ]
        ),
    ])
}

/// Dragging or clicking a slider: the value goes to its resource, and the slider is moved to
/// the rounded value at once, because a drag continues from where the slider says it is.
fn slider_changed(
    change: On<ValueChange<f32>>,
    controls: Query<&Control>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
    mut commands: Commands,
) {
    let Ok(&control) = controls.get(change.source) else {
        return;
    };
    let value = control.value_at(change.value);
    control.set(value, &mut playback, &mut settings);
    commands
        .entity(change.source)
        .insert(SliderValue(control.position_of(value)));
}

/// The wheel steps a slider to its next value.
fn slider_scrolled(
    mut scroll: On<Pointer<Scroll>>,
    controls: Query<&Control>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
    mut residue: Local<f32>,
) {
    let Ok(&control) = controls.get(scroll.entity) else {
        return;
    };
    scroll.propagate(false);
    // Touchpads scroll in small amounts; collect them into whole notches.
    *residue += wheel_notches(&scroll);
    let notches = residue.trunc();
    *residue -= notches;
    let mut value = control.get(&playback, &settings);
    for _ in 0..notches.abs() as u32 {
        value = control.nudged(value, notches > 0.0);
    }
    control.set(value, &mut playback, &mut settings);
}

/// Pushes resource state into the checkboxes and labels, so changes from anywhere (the widgets
/// themselves, shortcuts, the rule editor, the test rig) show up in the panel.
fn sync_widgets(
    universe: Res<Universe>,
    playback: Res<Playback>,
    settings: Res<Settings>,
    controls: Query<(Entity, &Does, Has<Checked>)>,
    mut readouts: Query<(&Readout, &mut Text)>,
    mut commands: Commands,
) {
    for (entity, does, checked) in &controls {
        let Action::Flip(toggle) = does.0 else {
            continue;
        };
        match (toggle.get(&playback, &settings, &universe), checked) {
            (true, false) => commands.entity(entity).insert(Checked),
            (false, true) => commands.entity(entity).remove::<Checked>(),
            _ => continue,
        };
    }
    for (readout, mut text) in &mut readouts {
        let content = match readout {
            Readout::PlayPauseLabel if playback.playing => "Pause".to_string(),
            Readout::PlayPauseLabel => "Play".to_string(),
            Readout::RuleName => universe.rule().name().to_string(),
            Readout::RuleBlurb => describe(universe.rule()),
            Readout::GridSize => format!("{} × {}", universe.width, universe.height),
            Readout::Generation | Readout::Transport | Readout::Population => continue,
        };
        text.set_if_neq(Text(content));
    }
}

/// Moves the sliders and their value labels to where the resources say they are.
fn sync_sliders(
    playback: Res<Playback>,
    settings: Res<Settings>,
    sliders: Query<(Entity, &Control, &SliderValue)>,
    mut labels: Query<(&ValueLabel, &mut Text)>,
    mut commands: Commands,
) {
    for (entity, control, current) in &sliders {
        let position = control.position_of(control.get(&playback, &settings));
        if (current.0 - position).abs() > 1e-6 {
            commands.entity(entity).insert(SliderValue(position));
        }
    }
    for (label, mut text) in &mut labels {
        let content = label.0.format(label.0.get(&playback, &settings));
        text.set_if_neq(Text(content));
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

/// The generation, what the transport is doing, and the population.
fn update_status(
    universe: Res<Universe>,
    playback: Res<Playback>,
    pace: Res<Pace>,
    mut readouts: Query<(&Readout, &mut Text)>,
) {
    if !(universe.is_changed() || playback.is_changed() || pace.is_changed()) {
        return;
    }
    let transport = if playback.playing {
        let direction = if playback.reverse { "backwards" } else { "forwards" };
        let requested = playback.speed * playback.stride as f32;
        match pace.achieved {
            // A few dropped frames are not worth a second number.
            Some(achieved) if achieved < 0.95 * requested => format!(
                "{direction} · {} of {} gen/s",
                format_rate(achieved),
                format_rate(requested)
            ),
            _ => format!("{direction} · {} gen/s", format_rate(requested)),
        }
    } else {
        "paused".to_string()
    };
    let cells = universe.width * universe.height;
    let population = universe.population();
    for (readout, mut text) in &mut readouts {
        let content = match readout {
            Readout::Generation => format!("generation {}", group_digits(universe.generation)),
            Readout::Transport => transport.clone(),
            Readout::Population => format!(
                "population {} ({:.2}%)",
                group_digits(population as i64),
                100.0 * population as f64 / cells as f64,
            ),
            _ => continue,
        };
        text.set_if_neq(Text(content));
    }
}

fn format_rate(generations_per_second: f32) -> String {
    if generations_per_second < 10.0 {
        format!("{generations_per_second:.1}")
    } else {
        group_digits(generations_per_second.round() as i64)
    }
}

/// `1234567` → `1 234 567`.
pub(crate) fn group_digits(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
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
    fn nudging_moves_in_the_direction_asked() {
        for control in [Control::Speed, Control::Stride, Control::Density] {
            let (lo, hi) = control.range();
            // From the bottom to the top and back, every nudge must make progress.
            let mut value = lo;
            let mut steps = 0;
            while value < hi {
                let next = control.nudged(value, true);
                assert!(next > value, "{control:?}: {value} went up to {next}");
                value = next;
                steps += 1;
            }
            assert!(steps > 20, "{control:?} has only {steps} stops");
            while value > lo {
                let next = control.nudged(value, false);
                assert!(next < value, "{control:?}: {value} went down to {next}");
                value = next;
            }
            assert_eq!(control.nudged(lo, false), lo);
            assert_eq!(control.nudged(hi, true), hi);
        }
        // A value between two stops, as halving the speed with a key leaves it.
        assert_eq!(Control::Speed.nudged(0.9375, false), 0.5);
        assert_eq!(Control::Speed.nudged(0.9375, true), 1.0);
        assert!([3.5, 4.0].contains(&Control::Speed.snap(3.75)));
        assert_eq!(Control::Stride.nudged(1.0, true), 2.0);
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
        assert_eq!(format_rate(122_880.0), "122 880");
        assert_eq!(format_rate(0.5), "0.5");
    }
}
