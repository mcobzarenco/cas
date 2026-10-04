//! The control panel (Bevy UI + feathers, dark theme) and the systems that keep it showing
//! the state of the simulation.
//!
//! The controls are sorted into cards by what they are about, their [`Aspect`]. Each aspect
//! has a colour, which comes back wherever that aspect shows up: in the side panels and on the
//! grid itself.

use bevy::{
    feathers::{
        constants::fonts,
        controls::{
            ButtonVariant, FeathersButton, FeathersMenu, FeathersMenuButton, FeathersMenuDivider, FeathersMenuItem,
            FeathersMenuPopup,
        },
        palette,
        theme::{ThemeBackgroundColor, ThemedText},
        tokens,
    },
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
    ui::Checked,
    ui_widgets::{Activate, ActivateOnPress, ScrollArea, SliderValue, ValueChange},
    window::PrimaryWindow,
};

use cas_core::{
    collection::Sort,
    rules::{BlockRule, PRESETS},
    universe::Universe,
};

use crate::{
    actions::{Action, Does, Toggle},
    analysis::{ANALYSIS_WIDTH, Analysis, ChoosingMark, SelectHint, analysis_panel},
    catcher::{CATCHER_WIDTH, Catcher, catcher_panel},
    editor::{EDITOR_WIDTH, RuleEditor, describe, editor_panel},
    kept::{Collected, KEPT_WIDTH, oscillators_panel, spaceships_panel, still_lifes_panel},
    kit::{
        self, Aspect, GUTTER, Scrolls, button, caption, checkbox, group_digits, key_hint, menu_heading, readout,
        scrolling,
    },
    library::{LIBRARY_WIDTH, RuleLibrary, library_panel},
    sim::{Pace, Playback, Settings, SimSystems, rule_changed},
    view::{grid_view, wheel_notches},
};

pub const PANEL_WIDTH: f32 = 300.0;
/// The grid is left this much of the window's width at least: side panels that would take
/// more are put away, the one opened longest ago first.
const GRID_ROOM: f32 = 320.0;

/// The grid sizes on offer: so many cells each way.
const GRID_SIDES: [usize; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];

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
    fn aspect(self) -> Aspect {
        match self {
            Control::Speed | Control::Stride => Aspect::Time,
            Control::Density => Aspect::Pattern,
        }
    }

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

/// The cards of the control panel, which scroll in a window too low for them.
#[derive(Component, Clone, Copy, Debug, Default)]
struct PanelBody;

/// The rule an item of the rule menu puts on the grid.
#[derive(Component, Clone, Debug)]
struct RuleChoice(BlockRule);

impl Default for RuleChoice {
    fn default() -> Self {
        Self(BlockRule::identity())
    }
}

/// Where the items of the rule menu are: they come and go with what is pinned in the library
/// and with the rules that were on the grid of late.
#[derive(Component, Default, Clone)]
struct RuleMenuItems;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_ui).add_systems(Update, make_room.in_set(SimSystems::Input)).add_systems(
            Update,
            (
                sync_widgets.run_if(rule_changed.or_eager(options_changed)),
                sync_sliders.run_if(options_changed),
                name_rule,
                list_rule_menu,
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
            library_panel(),
            editor_panel(),
            catcher_panel(),
            spaceships_panel(),
            oscillators_panel(),
            still_lifes_panel(),
            analysis_panel(),
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
            padding: px(GUTTER),
            row_gap: px(GUTTER),
        }
        Children [
            header(),
            // The cards scroll when the window is too low for them all.
            scrolling(Scrolls::Cards, bsn! {
                #PanelBody
                PanelBody
                Children [
                    rule_card(),
                    world_card(),
                    time_card(),
                    view_card(),
                    pattern_card(),
                ]
            }),
        ]
    }
}

/// The side panels by name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Library,
    Editor,
    Caught,
    Kept(Sort),
    Analysis,
}

/// Keeps room for the grid: when the side panels that are open would leave it less than
/// [`GRID_ROOM`], the ones opened longest ago are put away, down to the last one opened. In
/// a narrow window that is one panel at a time.
fn make_room(
    window: Single<&Window, With<PrimaryWindow>>,
    mut library: ResMut<RuleLibrary>,
    mut editor: ResMut<RuleEditor>,
    mut catcher: ResMut<Catcher>,
    mut collected: ResMut<Collected>,
    mut analysis: ResMut<Analysis>,
    mut open: Local<Vec<Side>>,
) {
    // The panels that are open, in the order they were opened.
    for (side, is_open) in [
        (Side::Library, library.is_open()),
        (Side::Editor, editor.is_open()),
        (Side::Caught, catcher.is_open()),
        (Side::Kept(Sort::Spaceship), collected.is_open(Sort::Spaceship)),
        (Side::Kept(Sort::Oscillator), collected.is_open(Sort::Oscillator)),
        (Side::Kept(Sort::StillLife), collected.is_open(Sort::StillLife)),
        (Side::Analysis, analysis.is_open()),
    ] {
        match (is_open, open.contains(&side)) {
            (true, false) => open.push(side),
            (false, true) => open.retain(|other| *other != side),
            _ => {}
        }
    }
    let width = |side: &Side| match side {
        Side::Library => LIBRARY_WIDTH,
        Side::Editor => EDITOR_WIDTH,
        Side::Caught => CATCHER_WIDTH,
        Side::Kept(_) => KEPT_WIDTH,
        Side::Analysis => ANALYSIS_WIDTH,
    };
    let room = window.width() - PANEL_WIDTH - GRID_ROOM;
    while open.len() > 1 && open.iter().map(width).sum::<f32>() > room {
        match open.remove(0) {
            Side::Library => library.close(),
            Side::Editor => editor.close(),
            Side::Caught => catcher.close(),
            Side::Kept(sort) => collected.close(sort),
            Side::Analysis => analysis.close(),
        }
    }
}

fn header() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Baseline,
            column_gap: px(8),
            padding: UiRect { left: px(12), top: px(2) },
        }
        Children [
            (
                Text("cas")
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(20.0),
                    weight: FontWeight::BOLD,
                }
                TextColor(palette::WHITE)
            ),
            caption("reversible block cellular automata"),
        ]
    }
}

/// A card of the control panel. `figure` is the number the card has to show, if any: a
/// readout next to its name.
fn card(aspect: Aspect, figure: Option<Readout>, body: impl SceneList) -> impl Scene {
    let figure: Box<dyn SceneList> = match figure {
        Some(figure) => bsn_list![(readout("") template_value(figure))].into(),
        None => bsn_list![].into(),
    };
    kit::card(aspect, figure, body)
}

/// A button that triggers `action`, labelled with its shortcut.
fn action_button(label: &'static str, name: &'static str, action: Action) -> impl Scene {
    action_button_hinted(label, name, action, action)
}

/// A button that triggers `action`, labelled with the shortcut of `hinted`: for a button
/// whose key does the same thing as far as a glance tells, and more besides.
fn action_button_hinted(label: &'static str, name: &'static str, action: Action, hinted: Action) -> impl Scene {
    let name = Name::new(name);
    let does = Does(action);
    bsn! {
        button(label, hinted.key())
        template_value(name)
        template_value(does)
    }
}

/// A checkbox for an on/off option, ticked in the colour of its aspect. Its checked state
/// always follows the resource, see [`sync_widgets`].
pub(crate) fn toggle(label: &'static str, name: &'static str, option: Toggle) -> impl Scene {
    let does = Does(Action::Flip(option));
    let (aspect, key) = (option.aspect(), Action::Flip(option).key());
    bsn! {
        checkbox(label, name, aspect, key)
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

/// The slider of a control, filled in the colour of its aspect: its value goes to the
/// control's resource, and the wheel steps it.
fn slider(name: &'static str, control: Control) -> impl Scene {
    let name = Name::new(name);
    bsn! {
        kit::slider(control.aspect())
        template_value(name)
        template_value(control)
        on(slider_changed)
        on(slider_scrolled)
    }
}

/// The rule menu, buttons for the library and the editor, and what the current rule does.
fn rule_card() -> impl Scene {
    card(
        Aspect::Rule,
        None,
        bsn_list![
            (
                @FeathersMenu
                Children [
                    (
                        #RuleMenu
                        @FeathersMenuButton {
                            @caption: bsn! { Text("") ThemedText template_value(Readout::RuleName) }
                        }
                        Node { flex_grow: 1.0 }
                    ),
                    (
                        // Its height is the window's business, which the kit sees to. Its
                        // items are the library's: see `list_rule_menu`.
                        @FeathersMenuPopup
                        Node { overflow: Overflow::scroll_y(), min_width: percent(100) }
                        ScrollArea
                        RuleMenuItems
                    ),
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    action_button("Library", "Library", Action::Library),
                    action_button("Edit", "EditRule", Action::EditRule),
                ]
            ),
            (caption("") template_value(Readout::RuleBlurb)),
        ],
    )
}

/// An item of the rule menu: a rule by the name it goes by. The rig knows it by `name`.
fn rule_item(name: String, label: String, rule: &BlockRule) -> impl Scene {
    let name = Name::new(name);
    let choice = RuleChoice(rule.clone());
    bsn! {
        @FeathersMenuItem {
            @caption: bsn! { Text(label) ThemedText }
        }
        Node { flex_shrink: 0.0 }
        template_value(name)
        template_value(choice)
        on(|activate: On<Activate>, choices: Query<&RuleChoice>, mut universe: ResMut<Universe>| {
            if let Ok(choice) = choices.get(activate.entity) {
                universe.set_rule(choice.0.clone());
            }
        })
    }
}

/// Keeps the rule menu to the few rules that are switched between: the ones pinned in the
/// library, and the latest of the others that were on the grid. Every other rule is a click
/// away in the library, which the last item opens.
fn list_rule_menu(
    library: Res<RuleLibrary>,
    popup: Single<Entity, With<RuleMenuItems>>,
    mut shown: Local<Option<u64>>,
    mut commands: Commands,
) {
    if shown.replace(library.revision()) == Some(library.revision()) {
        return;
    }
    let (pinned, recent) = library.offered();
    let mut items = Vec::new();
    if !pinned.is_empty() {
        items.push(commands.spawn_scene(menu_heading("PINNED")).id());
    }
    let mut kept = 0;
    for entry in pinned {
        // A built-in rule by its id, as ever; a kept one by its place among the pinned.
        let name = match PRESETS.iter().find(|preset| !entry.kept() && preset.table == *entry.rule.table()) {
            Some(preset) => format!("RuleItem:{}", preset.id),
            None => {
                kept += 1;
                format!("RulePinned{}", kept - 1)
            }
        };
        items.push(commands.spawn_scene(rule_item(name, entry.name.clone(), &entry.rule)).id());
    }
    if !recent.is_empty() {
        if !items.is_empty() {
            items.push(commands.spawn_scene(bsn! { @FeathersMenuDivider Node { flex_shrink: 0.0 } }).id());
        }
        items.push(commands.spawn_scene(menu_heading("OF LATE")).id());
    }
    for (place, rule) in recent.into_iter().enumerate() {
        items.push(commands.spawn_scene(rule_item(format!("RuleRecent{place}"), library.label(rule), rule)).id());
    }
    if !items.is_empty() {
        items.push(commands.spawn_scene(bsn! { @FeathersMenuDivider Node { flex_shrink: 0.0 } }).id());
    }
    let to_library = bsn! {
        #RuleItemLibrary
        @FeathersMenuItem {
            @caption: bsn! { Text("Library…") ThemedText }
        }
        Node { flex_shrink: 0.0 }
        on(|_: On<Activate>, mut library: ResMut<RuleLibrary>| library.show())
    };
    items.push(commands.spawn_scene(to_library).id());
    commands.entity(*popup).despawn_related::<Children>();
    commands.entity(*popup).add_children(&items);
}

/// The name of the rule on the grid and what it does, on the rule card: as the library has
/// them, or as its table tells.
fn name_rule(
    library: Res<RuleLibrary>,
    universe: Res<Universe>,
    mut readouts: Query<(&Readout, &mut Text)>,
    mut shown: Local<Option<(u64, BlockRule)>>,
) {
    let now = (library.revision(), universe.rule().clone());
    if shown.as_ref() == Some(&now) {
        return;
    }
    *shown = Some(now);
    let rule = universe.rule();
    let entry = library.entry(rule);
    for (readout, mut text) in &mut readouts {
        let content = match readout {
            Readout::RuleName => entry.map_or("Custom".to_string(), |entry| entry.name.clone()),
            // What was written of a kept rule, if anything was.
            Readout::RuleBlurb => match entry.filter(|entry| entry.kept() && !entry.note.is_empty()) {
                Some(entry) => entry.note.clone(),
                None => describe(rule),
            },
            _ => continue,
        };
        text.set_if_neq(Text(content));
    }
}

/// The space the automaton lives in: how big it is, and what its edge does.
fn world_card() -> impl Scene {
    let sizes: Vec<_> = GRID_SIDES.into_iter().map(size_item).collect();
    card(
        Aspect::World,
        None,
        bsn_list![
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
                                Node { overflow: Overflow::scroll_y() }
                                ScrollArea
                                Children [ { sizes } ]
                            ),
                        ]
                    ),
                ]
            ),
            toggle("Open border", "OpenBorder", Toggle::OpenBorder),
        ],
    )
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
        Node { flex_shrink: 0.0 }
        template_value(name)
        template_value(does)
    }
}

/// The transport and its pace; the generation it has got to is the card's figure.
fn time_card() -> impl Scene {
    // The step buttons act on the press, so that holding them can repeat (`repeat_steps`).
    let back = Does(Action::StepBack);
    let forward = Does(Action::StepForward);
    let play = Does(Action::PlayPause);
    let play_caption: Box<dyn SceneList> = bsn_list![
        (Text("Play") ThemedText template_value(Readout::PlayPauseLabel)),
        key_hint(Action::PlayPause.key()),
    ]
    .into();
    let keys = |down: Action, up: Action| format!("{} {}", down.key(), up.key());
    card(
        Aspect::Time,
        Some(Readout::Generation),
        bsn_list![
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
            // What the two sliders come to.
            (readout("") template_value(Readout::Transport)),
        ],
    )
}

fn view_card() -> impl Scene {
    let navigation = format!("wheel or {} {} zooms · right-drag pans", Action::ZoomIn.key(), Action::ZoomOut.key());
    card(
        Aspect::View,
        None,
        bsn_list![
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
        ],
    )
}

/// What is on the grid: seeding it, and catching the spaceships that reach its edge. How many
/// cells there are is the card's figure.
fn pattern_card() -> impl Scene {
    card(
        Aspect::Pattern,
        Some(Readout::Population),
        bsn_list![
            slider_row("Density", "Density", Control::Density, String::new()),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    action_button("Soup", "Soup", Action::Soup),
                    action_button("Blob", "Blob", Action::Blob),
                    action_button("Cloud", "Cloud", Action::Cloud),
                    action_button("Clear", "Clear", Action::Clear),
                ]
            ),
            caption("left-drag paints · with shift it erases"),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                }
                Children [
                    (
                        // Outlined in the pattern's colour while a pattern is being chosen.
                        action_button_hinted("Analyse", "Analyse", Action::Analyse, Action::Analysis)
                        Node { flex_grow: 0.0, border: px(1) }
                        BorderColor::all(Color::NONE)
                        ChoosingMark
                    ),
                    (
                        #SelectHint
                        caption("on the grid, or from the list")
                        SelectHint
                        Node { flex_grow: 1.0, flex_basis: px(0) }
                    ),
                ]
            ),
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
                        action_button("Caught", "Caught", Action::Caught)
                        Node { flex_grow: 0.0 }
                    ),
                ]
            ),
            (
                // What is kept under the rule, sort by sort. Three in a row: with less room
                // around their words.
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4),
                }
                Children [
                    (action_button("Spaceships", "Spaceships", Action::Kept(Sort::Spaceship)) Node { padding: UiRect::horizontal(px(4)) }),
                    (action_button("Oscillators", "Oscillators", Action::Kept(Sort::Oscillator)) Node { padding: UiRect::horizontal(px(4)) }),
                    (action_button("Still lifes", "StillLifes", Action::Kept(Sort::StillLife)) Node { padding: UiRect::horizontal(px(4)) }),
                ]
            ),
        ],
    )
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
    commands.entity(change.source).insert(SliderValue(control.position_of(value)));
}

/// The wheel steps a slider to its next value; unless the panel has to scroll, and then the
/// wheel is for that, wherever the pointer happens to be.
fn slider_scrolled(
    mut scroll: On<Pointer<Scroll>>,
    controls: Query<&Control>,
    panel: Single<&ComputedNode, With<PanelBody>>,
    mut playback: ResMut<Playback>,
    mut settings: ResMut<Settings>,
    mut residue: Local<f32>,
) {
    let Ok(&control) = controls.get(scroll.entity) else {
        return;
    };
    if panel.content_size().y > panel.size().y + 0.5 {
        return;
    }
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
            Readout::GridSize => format!("{} × {}", universe.width, universe.height),
            // The rule's name and what it does follow the library as well: see `name_rule`.
            Readout::RuleName | Readout::RuleBlurb => continue,
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

/// The figures of the cards (the generation, the population) and what the transport is doing.
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
            Some(achieved) if achieved < 0.95 * requested => {
                format!("{direction} · {} of {} gen/s", format_rate(achieved), format_rate(requested))
            }
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
                "{} {} ({:.2}%)",
                group_digits(population as i64),
                if population == 1 { "cell" } else { "cells" },
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
            assert!((back - value).abs() <= 1e-3 * value, "{control:?}: {value} came back as {back}");
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
        assert_eq!(format_rate(122_880.0), "122 880");
        assert_eq!(format_rate(0.5), "0.5");
    }
}
