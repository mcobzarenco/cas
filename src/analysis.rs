//! Analysing a pattern: a panel that runs a pattern of the grid on its own and says what it
//! does.
//!
//! A pattern is chosen by dragging a band around it on the grid (after **Analyse**), or sent
//! over from the spaceship list. It is followed on the unbounded plane until it repeats or
//! gets out of hand ([`Analyser::study`]), and shown living in a small world of its own, a
//! torus just big enough for it.

use std::sync::Arc;

use bevy::{
    clipboard::Clipboard,
    feathers::{
        constants::fonts,
        controls::{FeathersButton, FeathersScrollbar},
        theme::{ThemeTextColor, ThemedText},
        tokens,
    },
    platform::time::Instant,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    text::{FontSource, FontSourceTemplate, FontWeight},
    ui_widgets::{Activate, ControlOrientation, ScrollArea},
};

use cas_core::{
    census::Census,
    pattern::{
        Analyser, Cell, Fate, GROWING, Heading, Motion, Piece, PieceKind, SPREADING, Study, Symmetry, Turn, Watch,
        to_rle,
    },
    rules::BlockRule,
    universe::Universe,
};

use crate::{
    sim::{Settings, SimSystems},
    ui::{Aspect, caption, group_digits, panel_title, side_panel},
    view::{Framing, GridMaterial, GridParams, Stamp, cell_image, edge_of, upload},
};

pub const ANALYSIS_WIDTH: f32 = 396.0;

/// The size of the small world's view, in logical pixels; the world has the same proportions.
const VIEW: (f32, f32) = (360.0, 270.0);
/// The small world runs at this many generations a second at least, and faster for a pattern
/// with a long period, so that a period takes about this long; but no faster than this. A
/// pattern that never repeats is run at a pace of its own, to see soon what comes of it.
const PACE: (f32, f32) = (10.0, 120.0);
const PERIOD_SECONDS: f32 = 1.5;
const OPEN_PACE: f32 = 30.0;
/// So many kinds of spaceship that left the small world are named, the commonest first.
const KINDS_NAMED: usize = 3;
/// The small world is this many times as wide as the pattern gets, within these bounds, and
/// in any case wide enough for the pattern as it set out.
const ROOM: i32 = 3;
const WORLD_SIDES: (usize, usize) = (32, 256);
/// How far a pattern is followed, whatever it is: for so many generations; or until it has so
/// many times the cells it set out with, or is so many times as wide, which for a small
/// pattern is so many cells and so wide at least.
const GENERATIONS: u32 = 8192;
const MOST_CELLS: (usize, usize) = (4, 1024);
const WIDEST: (i32, i32) = (2, 512);
/// A study is made on another thread, and most are done before the next frame. One that is
/// still on its way after so many seconds shows in the panel, with how far it has got.
const SHOWS_AFTER: f32 = 0.15;
/// The small world's clock makes up for no more than so many seconds of a frame that took
/// long: a window that was out of sight for an hour does not run an hour of generations.
const LONGEST_FRAME: f32 = 0.25;
/// So many characters of the pattern's text are shown; Copy copies all of it.
const TEXT_SHOWN: usize = 36;

#[derive(Resource, Default)]
pub struct Analysis {
    open: bool,
    /// A pattern is being chosen on the grid: the next left-drag draws a band around it.
    pub selecting: bool,
    /// The band, from the cell pressed to the cell under the pointer.
    pub band: Option<(IVec2, IVec2)>,
    subject: Option<Subject>,
    /// The study that is on its way, on another thread.
    studying: Option<Studying>,
    /// How many patterns were studied: numbers the subjects, which tells a new one from the
    /// one before.
    studied: u64,
    /// What the last button did.
    note: Option<String>,
}

/// The pattern under study.
struct Subject {
    number: u64,
    study: Study,
    rule: BlockRule,
    /// The pattern through the vacuum's cycle, to put it back on the grid.
    forms: Vec<Vec<Cell>>,
    /// The small world it lives in, how many generations that world is owed, and how many it
    /// gets a second.
    world: Universe,
    clock: f32,
    pace: f32,
    paused: bool,
    /// What left the small world, for a pattern that never repeats: its border is open then,
    /// and what reaches it is caught and told apart.
    census: Option<Census>,
    rle: String,
}

/// A study on its way: how many cells are being followed and under which rule, since when,
/// the watch it is seen through and stopped by, and the task that will have the subject.
struct Studying {
    cells: usize,
    rule: BlockRule,
    began: Instant,
    watch: Arc<Watch>,
    task: Task<Option<Subject>>,
}

impl Subject {
    /// Studies a pattern and gives it a small world to live in: the work of a study, done
    /// where this is called. None for no cells at all.
    fn of(number: u64, cells: &[Cell], phase: usize, rule: BlockRule, watch: Arc<Watch>) -> Option<Self> {
        let (width, height) = bounding_box(cells);
        let mut analyser = Analyser::new(&rule);
        analyser.max_generations = GENERATIONS;
        analyser.max_cells = (MOST_CELLS.0 * cells.len()).max(MOST_CELLS.1);
        analyser.max_extent = (WIDEST.0 * width.max(height)).max(WIDEST.1);
        analyser.watch = Some(watch);
        let study = analyser.study(cells, phase)?;
        let pace = match &study.motion {
            Some(motion) => (motion.period as f32 / PERIOD_SECONDS).clamp(PACE.0, PACE.1),
            None => OPEN_PACE,
        };
        let mut world = small_world(&study, &rule);
        // A pattern that repeats is left to go round its torus. One that does not would fill
        // it: what it sends out leaves through an open border instead, and is counted.
        let census = (study.motion.is_none()).then(|| {
            world.open_border = true;
            world.catching = true;
            Census::with(Analyser::new(&rule))
        });
        Some(Self {
            number,
            forms: analyser.forms(&study.start),
            world,
            rle: to_rle(&study.start),
            rule,
            clock: 0.0,
            pace,
            paused: false,
            census,
            study,
        })
    }

    /// The small world as it was when the study began: the pattern as it set out, at
    /// generation 0, and nothing caught yet.
    fn restart(&mut self) {
        self.world = small_world(&self.study, &self.rule);
        self.clock = 0.0;
        if let Some(census) = &mut self.census {
            self.world.open_border = true;
            self.world.catching = true;
            *census = Census::with(Analyser::new(&self.rule));
        }
    }
}

impl Analysis {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.stop_choosing();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        if self.open {
            self.toggle();
        }
    }

    /// Choosing a pattern on the grid begins, or is called off; it opens the panel, which
    /// says what to do.
    pub fn choose(&mut self) {
        if self.selecting {
            self.stop_choosing();
        } else {
            self.start_choosing();
        }
    }

    pub fn start_choosing(&mut self) {
        self.selecting = true;
        self.band = None;
        self.note = None;
        self.open = true;
    }

    pub fn stop_choosing(&mut self) {
        if self.selecting || self.band.is_some() {
            self.selecting = false;
            self.band = None;
        }
    }

    /// Studies a pattern, given relative to a corner of the blocks the next step rewrites, as
    /// a pattern caught at the edge is, with the vacuum `phase` generations into its cycle.
    /// The study is made on another thread, and its pattern is on display when it is done
    /// ([`take_study`]): a large pattern, or one that takes long to make up its mind, does
    /// not hold up the frames.
    pub fn study(&mut self, cells: Vec<Cell>, phase: usize, universe: &Universe) {
        // Choosing stays on, for the next pattern.
        self.band = None;
        self.open = true;
        // A study still on its way is not waited for: it is told to stop, and let go of.
        if let Some(studying) = self.studying.take() {
            studying.watch.stop();
        }
        if cells.is_empty() {
            self.note = Some("No live cells in there.".to_string());
            return;
        }
        self.note = None;
        self.studied += 1;
        let (number, size) = (self.studied, cells.len());
        let (rule, watch) = (universe.rule().clone(), Arc::new(Watch::default()));
        let task = AsyncComputeTaskPool::get().spawn({
            let (rule, watch) = (rule.clone(), watch.clone());
            async move { Subject::of(number, &cells, phase, rule, watch) }
        });
        self.studying = Some(Studying { cells: size, rule, began: Instant::now(), watch, task });
    }

    /// The study on its way, once it has been so for long enough to show.
    fn busy(&self) -> Option<&Studying> {
        self.studying.as_ref().filter(|studying| studying.began.elapsed().as_secs_f32() >= SHOWS_AFTER)
    }

    /// The pattern on display: the one last studied, unless a study on its way shows instead.
    fn shown(&self) -> Option<&Subject> {
        self.subject.as_ref().filter(|_| self.busy().is_none())
    }
}

/// A torus with room for the pattern, the pattern in the middle of it. The world is at
/// generation 0, so the pattern sits on an even corner, as its coordinates have it.
fn small_world(study: &Study, rule: &BlockRule) -> Universe {
    let (width, height) = bounding_box(&study.start);
    let room = (ROOM * study.extent.0.max(study.extent.1)) as usize;
    let side = room.clamp(WORLD_SIDES.0, WORLD_SIDES.1).max(width.max(height) as usize + 2);
    let (rows, columns) = (side.next_multiple_of(2), (side * 4 / 3).next_multiple_of(2));
    let mut world = Universe::new(columns, rows, rule.clone());
    let (x0, y0) = (((columns as i32 - width) / 2) & !1, ((rows as i32 - height) / 2) & !1);
    for &(x, y) in &study.start {
        world.set((x0 + x) as usize, (y0 + y) as usize, true);
    }
    world
}

/// Width and height of the pattern's bounding box.
fn bounding_box(cells: &[Cell]) -> (i32, i32) {
    let span = |axis: fn(&Cell) -> i32| {
        let (min, max) = cells.iter().map(axis).fold((i32::MAX, i32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        if min > max { 0 } else { max - min + 1 }
    };
    (span(|cell| cell.0), span(|cell| cell.1))
}

/// The panel.
#[derive(Component, Default, Clone)]
struct AnalysisPanel;

/// The node that shows the small world.
#[derive(Component, Default, Clone)]
struct SmallView;

/// The line under the small world: how big it is and how fast it runs.
#[derive(Component, Default, Clone)]
struct SmallCaption;

/// The name of the rule the pattern was studied in, next to the title.
#[derive(Component, Default, Clone)]
struct SubjectRule;

/// One thing the study says.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Finding {
    #[default]
    What,
    Period,
    Speed,
    Cells,
    Changes,
    Size,
    Symmetry,
    Pieces,
    Text,
}

/// The line under the small world's caption: its generation, and what has left it.
#[derive(Component, Default, Clone)]
struct SmallStatus;

/// The caption of the Pause button, which says Play while the small world stands still.
#[derive(Component, Default, Clone)]
struct PauseLabel;

/// The line under the buttons.
#[derive(Component, Default, Clone)]
struct Note;

/// When a control of the small world is there: while a pattern is at rest in it, or while a
/// study is on its way.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
enum During {
    #[default]
    Rest,
    Study,
}

/// The caption next to the Analyse button of the pattern card: what to do next.
#[derive(Component, Default, Clone)]
pub struct SelectHint;

/// The Analyse button of the pattern card, which is outlined while a pattern is being chosen:
/// a drag on the grid then draws a band, and does not paint.
#[derive(Component, Default, Clone)]
pub struct ChoosingMark;

/// The texture and the material the small world is drawn with, and an empty world to draw
/// while there is no pattern.
#[derive(Resource)]
struct SmallAssets {
    image: Handle<Image>,
    material: Handle<GridMaterial>,
    empty: Universe,
}

impl FromWorld for SmallAssets {
    fn from_world(world: &mut World) -> Self {
        let empty = Universe::new(WORLD_SIDES.0, WORLD_SIDES.0, BlockRule::identity());
        let image = world.resource_mut::<Assets<Image>>().add(cell_image(&empty));
        let material = world.resource_mut::<Assets<GridMaterial>>().add(GridMaterial::new(image.clone()));
        Self { image, material, empty }
    }
}

pub struct AnalysisPlugin;

impl Plugin for AnalysisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Analysis>()
            .init_resource::<SmallAssets>()
            .add_observer(attach_material)
            .add_systems(Update, call_off.in_set(SimSystems::Input))
            .add_systems(
                Update,
                (take_study, run_small_world, draw_small_world, sync_panel, show_status, label_pause)
                    .chain()
                    .in_set(SimSystems::Present),
            );
    }
}

fn attach_material(add: On<Add, SmallView>, assets: Res<SmallAssets>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(MaterialNode(assets.material.clone()));
}

pub fn analysis_panel() -> impl Scene {
    bsn! {
        #Analysis
        side_panel(ANALYSIS_WIDTH, bsn_list![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                }
                Children [
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Baseline,
                            column_gap: px(10),
                        }
                        Children [
                            panel_title(Aspect::Pattern, "Analysis"),
                            // The rule the pattern was studied in, which the grid may have left.
                            (#SubjectRule caption("") SubjectRule),
                        ]
                    ),
                    (
                        #AnalysisClose
                        @FeathersButton {
                            @caption: bsn! { Text("Close") ThemedText }
                        }
                        on(|_: On<Activate>, mut analysis: ResMut<Analysis>| analysis.toggle())
                    ),
                ]
            ),
            (
                // The frame holds the scrollbar, in the margin of the panel; what is in it
                // scrolls when the window is too low for it.
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                }
                Children [
                    (
                        #AnalysisBody
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(12),
                            overflow: Overflow::scroll_y(),
                        }
                        ScrollArea
                        Children [
                        caption("A pattern on its own, followed until it repeats or gets out of hand, and left to live in a small world. Choose one with Analyse and a drag over the grid, or send a spaceship over from the list."),
                        (
                            #AnalysisView
                            Node {
                                width: px(VIEW.0),
                                height: px(VIEW.1),
                                flex_shrink: 0.0,
                            }
                            SmallView
                        ),
                        (
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(6),
                            }
                            Children [
                                (
                                    #SmallCaption
                                    caption("")
                                    SmallCaption
                                    Node { flex_grow: 1.0, flex_basis: px(0) }
                                ),
                                (
                                    #AnalysisPause
                                    @FeathersButton {
                                        @caption: bsn! { Text("Pause") ThemedText PauseLabel }
                                    }
                                    Node { flex_shrink: 0.0, min_height: px(22), padding: UiRect::axes(px(8), px(0)) }
                                    template_value(During::Rest)
                                    on(pause_world)
                                ),
                                (
                                    #AnalysisRestart
                                    @FeathersButton {
                                        @caption: bsn! { Text("Restart") ThemedText }
                                    }
                                    Node { flex_shrink: 0.0, min_height: px(22), padding: UiRect::axes(px(8), px(0)) }
                                    template_value(During::Rest)
                                    on(restart_world)
                                ),
                                (
                                    // While a study is on its way: far enough.
                                    #AnalysisStop
                                    @FeathersButton {
                                        @caption: bsn! { Text("Stop") ThemedText }
                                    }
                                    Node {
                                        display: Display::None,
                                        flex_shrink: 0.0,
                                        min_height: px(22),
                                        padding: UiRect::axes(px(8), px(0)),
                                    }
                                    template_value(During::Study)
                                    on(stop_study)
                                ),
                            ]
                        ),
                        (
                            // The small world's generation, and what left it: in the mono font, which has
                            // the arrows.
                            #SmallStatus
                            Text("")
                            TextFont {
                                font: FontSourceTemplate::Handle(fonts::MONO),
                                font_size: FontSize::Px(12.0),
                                weight: FontWeight::NORMAL,
                            }
                            ThemeTextColor(tokens::TEXT_DIM)
                            SmallStatus
                        ),
                        (
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(5),
                            }
                            Children [
                                line("WHAT", Finding::What),
                                line("PERIOD", Finding::Period),
                                line("SPEED", Finding::Speed),
                                line("CELLS", Finding::Cells),
                                line("CHANGES", Finding::Changes),
                                line("SIZE", Finding::Size),
                                line("SYMMETRY", Finding::Symmetry),
                                line("PIECES", Finding::Pieces),
                                line("TEXT", Finding::Text),
                            ]
                        ),
                        (
                            Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: px(6),
                            }
                            Children [
                                (
                                    #AnalysisPlace
                                    @FeathersButton {
                                        @caption: bsn! { Text("Place") ThemedText }
                                    }
                                    Node { flex_grow: 1.0 }
                                    on(place_subject)
                                ),
                                (
                                    #AnalysisCopy
                                    @FeathersButton {
                                        @caption: bsn! { Text("Copy") ThemedText }
                                    }
                                    Node { flex_grow: 1.0 }
                                    on(copy_subject)
                                ),
                            ]
                        ),
                        (#AnalysisNote caption("") Note),
                        ]
                    ),
                    (
                        @FeathersScrollbar {
                            @target: #AnalysisBody,
                            @orientation: {ControlOrientation::Vertical}
                        }
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            right: px(-10),
                            top: px(0),
                            bottom: px(0),
                            width: px(6),
                        }
                    ),
                ]
            ),
        ])
        AnalysisPanel
    }
}

/// One finding: its name and, next to it, what was found.
fn line(label: &'static str, finding: Finding) -> impl Scene {
    let name = Name::new(format!("Study{finding:?}"));
    let font = if in_mono(finding, "") { fonts::MONO } else { fonts::REGULAR };
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::FlexStart,
            column_gap: px(10),
        }
        Children [
            (
                Text(label)
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(10.0),
                    weight: FontWeight::BOLD,
                }
                ThemeTextColor(tokens::TEXT_DIM)
                Node {
                    width: px(66),
                    flex_shrink: 0.0,
                    // Level with the first line of what was found, which is set larger.
                    margin: UiRect::top(px(3)),
                }
            ),
            (
                Text("—")
                TextFont {
                    font: FontSourceTemplate::Handle(font),
                    font_size: FontSize::Px(12.0),
                    weight: FontWeight::NORMAL,
                }
                ThemeTextColor(tokens::TEXT_MAIN)
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                }
                template_value(name)
                template_value(finding)
            ),
        ]
    }
}

/// The pattern goes back on the grid: picked up, to be put down wherever. Under another rule
/// than it was studied in, the same cells go down, filed for that rule's vacuum.
fn place_subject(
    _: On<Activate>,
    universe: Res<Universe>,
    mut analysis: ResMut<Analysis>,
    mut stamp: ResMut<Stamp>,
) {
    if let Some(subject) = analysis.shown() {
        let forms = if universe.rule() == &subject.rule {
            subject.forms.clone()
        } else {
            Analyser::new(universe.rule()).forms(&subject.study.start)
        };
        stamp.pick_up(forms, universe.rule(), None);
        analysis.note = None;
        // A click on the grid puts the pattern down now, rather than starting a band.
        analysis.stop_choosing();
    }
}

/// The small world stands still, or runs again.
fn pause_world(_: On<Activate>, mut analysis: ResMut<Analysis>) {
    if let Some(subject) = analysis.subject.as_mut() {
        subject.paused = !subject.paused;
    }
}

/// The small world starts over from the pattern as it set out.
fn restart_world(_: On<Activate>, mut analysis: ResMut<Analysis>) {
    if let Some(subject) = analysis.subject.as_mut() {
        subject.restart();
    }
}

fn copy_subject(_: On<Activate>, mut analysis: ResMut<Analysis>, mut clipboard: ResMut<Clipboard>) {
    let Some(rle) = analysis.shown().map(|subject| subject.rle.clone()) else {
        return;
    };
    analysis.note = Some(match clipboard.set_text(rle.as_str()) {
        Ok(()) => format!("Copied {rle}"),
        Err(error) => format!("The clipboard is not available ({error:?})."),
    });
}

/// Far enough: the study on its way is told to stop, and what it knows by then is the study.
fn stop_study(_: On<Activate>, analysis: Res<Analysis>) {
    if let Some(studying) = &analysis.studying {
        studying.watch.stop();
    }
}

/// Takes the study that is done: its pattern is the one on display from now on.
fn take_study(mut analysis: ResMut<Analysis>) {
    // Looking is no change: only a study that is done is.
    let studying = analysis.bypass_change_detection().studying.as_mut();
    let Some(subject) = studying.and_then(|studying| check_ready(&mut studying.task)) else {
        return;
    };
    analysis.studying = None;
    match subject {
        Some(subject) => analysis.subject = Some(subject),
        None => analysis.note = Some("No live cells in there.".to_string()),
    }
}

/// Escape calls the choosing off.
fn call_off(keys: Res<ButtonInput<KeyCode>>, mut analysis: ResMut<Analysis>) {
    if keys.just_pressed(KeyCode::Escape) && (analysis.selecting || analysis.band.is_some()) {
        analysis.stop_choosing();
    }
}

/// The small world runs at its own pace while the panel is open. Nobody watches the resource
/// for this, so the change goes unannounced.
fn run_small_world(time: Res<Time<Real>>, mut analysis: ResMut<Analysis>) {
    if !analysis.open || analysis.busy().is_some() {
        return;
    }
    if let Some(subject) = analysis.bypass_change_detection().subject.as_mut()
        && !subject.paused
    {
        subject.clock += time.delta_secs().min(LONGEST_FRAME) * subject.pace;
        let steps = subject.clock.floor();
        if steps >= 1.0 {
            subject.world.step_by(steps as i64);
            subject.clock -= steps;
            if let Some(census) = subject.census.as_mut() {
                for departure in subject.world.take_departures() {
                    census.record(departure);
                }
            }
        }
    }
}

/// The small world's view, drawn as the grid is: the whole world fitted into the node.
fn draw_small_world(
    analysis: Res<Analysis>,
    settings: Res<Settings>,
    assets: Res<SmallAssets>,
    node: Single<&ComputedNode, With<SmallView>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<GridMaterial>>,
) {
    if !analysis.open {
        return;
    }
    let world = analysis.shown().map_or(&assets.empty, |subject| &subject.world);
    let size = node.size * node.inverse_scale_factor;
    if size.min_element() < 1.0 {
        return;
    }
    if let Some(mut image) = images.get_mut(&assets.image) {
        upload(world, &mut image);
    }
    let framing = Framing::fitted(world, size, 1.0 / node.inverse_scale_factor);
    let params = GridParams::new(world, framing, &settings, edge_of(world), None, None);
    GridMaterial::set(&mut materials, &assets.material, params);
}

/// What the panel last showed, to tell when it is out of date.
#[derive(PartialEq)]
struct Shown {
    subject: Option<u64>,
    /// The study on its way that shows instead, by its number.
    busy: Option<u64>,
    selecting: bool,
    /// The subject is picked up, to be put down on the grid.
    holding: bool,
    note: Option<String>,
}

/// Shows or hides the panel, and keeps its texts and the hint of the pattern card in step.
fn sync_panel(
    analysis: Res<Analysis>,
    stamp: Res<Stamp>,
    mut panel: Single<&mut Node, (With<AnalysisPanel>, Without<During>)>,
    mut controls: Query<(&During, &mut Node), Without<AnalysisPanel>>,
    mut findings: Query<(&Finding, &mut Text, &mut TextFont)>,
    assets: Res<AssetServer>,
    mut captions: Query<&mut Text, (With<SmallCaption>, Without<Finding>, Without<Note>, Without<SelectHint>)>,
    mut rule_name: Single<&mut Text, (With<SubjectRule>, Without<SmallCaption>, Without<Finding>, Without<Note>, Without<SelectHint>)>,
    mut note: Single<&mut Text, (With<Note>, Without<Finding>, Without<SelectHint>)>,
    mut hint: Single<&mut Text, (With<SelectHint>, Without<Finding>, Without<Note>)>,
    mut mark: Single<&mut BorderColor, With<ChoosingMark>>,
    mut shown: Local<Option<Shown>>,
) {
    let display = if analysis.open { Display::Flex } else { Display::None };
    if panel.display != display {
        panel.display = display;
    }
    let busy = analysis.busy();
    let now = Shown {
        subject: analysis.subject.as_ref().map(|subject| subject.number),
        busy: busy.map(|_| analysis.studied),
        selecting: analysis.selecting,
        // A stamp from the list is the list's business.
        holding: stamp.is_held() && stamp.kind.is_none(),
        note: analysis.note.clone(),
    };
    if shown.as_ref() == Some(&now) {
        return;
    }
    let now = shown.insert(now);

    // The controls of the small world, or the one that stops a study on its way.
    for (during, mut node) in &mut controls {
        let display = if (*during == During::Study) == busy.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let subject = analysis.shown();
    for (finding, mut text, mut font) in &mut findings {
        let content = match (subject, busy, finding) {
            (Some(subject), ..) => found(*finding, subject),
            (None, Some(_), Finding::What) => "Being followed on its own…".to_string(),
            _ => "—".to_string(),
        };
        let face = if in_mono(*finding, &content) { fonts::MONO } else { fonts::REGULAR };
        if text.set_if_neq(Text(content)) {
            font.font = FontSource::Handle(assets.load(face));
        }
    }
    for mut text in &mut captions {
        let content = match (subject, busy) {
            (Some(subject), _) => {
                let (width, height) = (subject.world.width, subject.world.height);
                format!("{width}×{height} cells, {} generations a second", subject.pace.round())
            }
            (None, Some(studying)) => {
                let cells = if studying.cells == 1 { "cell" } else { "cells" };
                format!("{} {cells}, for {} at most", count(studying.cells), generations(GENERATIONS))
            }
            _ => String::new(),
        };
        text.set_if_neq(Text(content));
    }
    let rule = subject.map(|subject| &subject.rule).or(busy.map(|studying| &studying.rule));
    rule_name.set_if_neq(Text(rule.map_or("", |rule| rule.name()).to_string()));
    let what_next = match (analysis.selecting, subject.is_some(), now.holding) {
        _ if busy.is_some() => "Stop takes what is known by then for the study. A drag over another pattern studies that one instead.",
        (_, _, true) => "Click the grid to put the pattern down, as often as you like. Escape or a right click lets go of it.",
        (true, false, _) => "Drag over the pattern on the grid. Escape calls it off.",
        (true, true, _) => "Drag over another pattern, or Place picks this one up to be put down on the grid where you click.",
        (false, false, _) => "Nothing yet.",
        (false, true, _) => "Place picks the pattern up, to be put down on the grid where you click.",
    };
    note.set_if_neq(Text(analysis.note.clone().unwrap_or(what_next.to_string())));
    let hint_text = if analysis.selecting { "drag over it · Escape cancels" } else { "on the grid, or from the list" };
    hint.set_if_neq(Text(hint_text.to_string()));
    let outline = if analysis.selecting { Aspect::Pattern.color() } else { Color::NONE };
    mark.set_if_neq(BorderColor::all(outline));
}

/// The line under the small world: where it has got to, and what has left it. It changes as
/// the world runs.
fn show_status(analysis: Res<Analysis>, mut status: Single<&mut Text, With<SmallStatus>>) {
    if !analysis.open {
        return;
    }
    let said = match (analysis.busy(), analysis.shown()) {
        // A study on its way: how far it has got.
        (Some(studying), _) => {
            let generation = format!("generation {}", group_digits(studying.watch.generation() as i64));
            if studying.watch.stopped() {
                format!("{generation}, stopping")
            } else if studying.watch.taking_apart() {
                format!("{generation}, and what it became is being taken apart")
            } else {
                generation
            }
        }
        (None, Some(subject)) => {
            let generation = format!("generation {}", subject.world.generation);
            match left_so_far(subject) {
                Some(left) => format!("{generation}, {left}"),
                None => generation,
            }
        }
        (None, None) => String::new(),
    };
    status.set_if_neq(Text(said));
}

/// The Pause button says what it would do.
fn label_pause(analysis: Res<Analysis>, mut label: Single<&mut Text, With<PauseLabel>>) {
    let paused = analysis.subject.as_ref().is_some_and(|subject| subject.paused);
    label.set_if_neq(Text(if paused { "Play" } else { "Pause" }.to_string()));
}

/// What the study says, in words.
fn found(finding: Finding, subject: &Subject) -> String {
    let study = &subject.study;
    let motion = study.motion.as_ref();
    let travels = motion.is_some_and(|motion| motion.heading() != Heading::Still);
    match finding {
        Finding::What => match (study.still, travels, study.fate) {
            (true, ..) => "Still life".to_string(),
            (_, true, _) => "Spaceship".to_string(),
            (_, _, Fate::Returns { .. }) => "Oscillator".to_string(),
            // A gun's streams get too wide before they are too many cells: what grows while
            // it flies apart grows.
            (_, _, Fate::Grows | Fate::Scatters) if study.growth.is_some_and(|growth| growth >= GROWING) => {
                let how = if study.growth >= Some(SPREADING) { "over the plane" } else { "along lines, as a gun does" };
                format!("Grows {how}: {} cells after {}", count(study.cells.1), generations(study.generations))
            }
            (_, _, Fate::Grows) => format!("Grows: {} cells after {}", count(study.cells.1), generations(study.generations)),
            (_, _, Fate::Scatters) => format!(
                "Flies apart: {} cells across after {}",
                study.extent.0.max(study.extent.1),
                generations(study.generations)
            ),
            (_, _, Fate::Undecided) => format!("Undecided after {}", generations(study.generations)),
        },
        Finding::Period => {
            let sooner = match study.recurs {
                Some((after, turn)) if !study.still => format!(" ({} after {after})", turned(turn)),
                _ => String::new(),
            };
            match (study.still, study.fate) {
                (true, _) => generations(1),
                (false, Fate::Returns { period, displacement: (dx, dy) }) if travels => {
                    format!("{}{sooner}, moving ({dx}, {dy})", generations(period))
                }
                (false, Fate::Returns { period, .. }) => format!("{}{sooner}", generations(period)),
                _ => "—".to_string(),
            }
        }
        Finding::Speed => match (motion, study.fate) {
            // The motion is that of the canonical form; the way it goes is as it lies.
            (Some(motion), Fate::Returns { displacement: (dx, dy), .. }) if travels => {
                format!("{} {} {}", speed(motion), heading(motion), arrow(dx, dy))
            }
            _ => "—".to_string(),
        },
        Finding::Cells => match study.cells {
            (fewest, most) if fewest == most => count(fewest),
            (fewest, most) => format!("{} to {}", count(fewest), count(most)),
        },
        Finding::Changes => match study.fate {
            _ if study.still => "none".to_string(),
            Fate::Returns { displacement: (0, 0), .. } if study.stator > 0 => {
                format!("{:.1} cells a generation, {} never change", study.heat, study.stator)
            }
            Fate::Returns { .. } => format!("{:.1} cells a generation", study.heat),
            _ => "—".to_string(),
        },
        Finding::Size => {
            let (width, height) = bounding_box(&study.start);
            if (width, height) == study.extent {
                format!("{width}×{height}")
            } else {
                format!("{width}×{height}, up to {}×{}", study.extent.0, study.extent.1)
            }
        }
        Finding::Symmetry => match study.symmetry {
            Symmetry::None => "none",
            Symmetry::Mirror => "a mirror",
            Symmetry::DiagonalMirror => "a mirror across a diagonal",
            Symmetry::HalfTurn => "a half turn",
            Symmetry::TwoMirrors => "mirrors both ways, so a half turn",
            Symmetry::TwoDiagonalMirrors => "mirrors across both diagonals, so a half turn",
            Symmetry::QuarterTurn => "a quarter turn",
            Symmetry::All => "every turn and mirror",
        }
        .to_string(),
        Finding::Pieces => match study.fate {
            Fate::Returns { .. } => match study.parts {
                1 => "one piece".to_string(),
                parts => format!("{parts} that never meet"),
            },
            _ if study.pieces.is_empty() => "—".to_string(),
            _ => pieces(&study.pieces, study.more_pieces),
        },
        Finding::Text => match subject.rle.char_indices().nth(TEXT_SHOWN) {
            Some((end, _)) => format!("{}…", &subject.rle[..end]),
            None => subject.rle.clone(),
        },
    }
}

/// What is set in the mono font: the text of the pattern, and whatever points a way, since
/// the mono font is the one with the diagonal arrows.
fn in_mono(finding: Finding, text: &str) -> bool {
    matches!(finding, Finding::Speed | Finding::Text) || text.contains(['↖', '↑', '↗', '←', '→', '↙', '↓', '↘'])
}

fn generations(n: u32) -> String {
    format!("{} generation{}", group_digits(n as i64), if n == 1 { "" } else { "s" })
}

/// A number of cells, as it is written everywhere in the panel.
fn count(cells: usize) -> String {
    group_digits(cells as i64)
}

fn turned(turn: Turn) -> &'static str {
    match turn {
        Turn::Quarter => "turned a quarter",
        Turn::Half => "turned about",
        Turn::Mirror => "mirrored",
        Turn::DiagonalMirror => "mirrored across a diagonal",
    }
}

/// A speed as a fraction of the speed of light.
fn speed(motion: &Motion) -> String {
    match motion.speed() {
        (1, 1) => "c".to_string(),
        (1, period) => format!("c/{period}"),
        (cells, period) => format!("{cells}c/{period}"),
    }
}

fn heading(motion: &Motion) -> &'static str {
    match motion.heading() {
        Heading::Orthogonal => "orthogonal",
        Heading::Diagonal => "diagonal",
        _ => "oblique",
    }
}

/// What a pattern came apart into, counted by what the pieces are: spaceships by their speed
/// and way, oscillators by their period, and the rest by what became of them.
fn pieces(pieces: &[Piece], more: usize) -> String {
    let mut ships: Vec<(String, usize)> = Vec::new();
    let mut periods: Vec<(u32, usize)> = Vec::new();
    let (mut still, mut grow, mut scatter, mut unsettled, mut big) = (0, 0, 0, 0, 0);
    for piece in pieces {
        match piece.kind {
            PieceKind::Spaceship { period, displacement } => {
                let motion = Motion { period, displacement, canonical: Vec::new() };
                let name = format!("{} {}", speed(&motion), arrow(displacement.0, displacement.1));
                tally(&mut ships, name);
            }
            PieceKind::Oscillator { period } => tally(&mut periods, period),
            PieceKind::StillLife => still += 1,
            PieceKind::Grows => grow += 1,
            PieceKind::Scatters => scatter += 1,
            PieceKind::Undecided => unsettled += 1,
            PieceKind::Unexamined => big += 1,
        }
    }
    let mut said = Vec::new();
    if !ships.is_empty() {
        ships.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let flown: usize = ships.iter().map(|(_, count)| count).sum();
        let mut kinds: Vec<String> = ships
            .iter()
            .take(KINDS_NAMED)
            .map(|(name, count)| if *count > 1 { format!("{name} ×{count}") } else { name.clone() })
            .collect();
        if ships.len() > KINDS_NAMED {
            kinds.push(counted(ships.len() - KINDS_NAMED, "one more kind", "more kinds"));
        }
        said.push(format!("{} ({})", counted(flown, "spaceship", "spaceships"), kinds.join(", ")));
    }
    if !periods.is_empty() {
        periods.sort();
        let swinging: usize = periods.iter().map(|(_, count)| count).sum();
        let named: Vec<String> = periods.iter().map(|(period, _)| period.to_string()).collect();
        let of = if periods.len() == 1 { "period" } else { "periods" };
        said.push(format!("{} ({of} {})", counted(swinging, "oscillator", "oscillators"), named.join(", ")));
    }
    if still > 0 {
        said.push(counted(still, "still life", "still lifes"));
    }
    if grow > 0 {
        said.push(counted(grow, "one that grows", "that grow"));
    }
    if scatter > 0 {
        said.push(counted(scatter, "one that flies apart", "that fly apart"));
    }
    if unsettled > 0 {
        said.push(counted(unsettled, "one still changing", "still changing"));
    }
    if big > 0 {
        said.push(counted(big, "one too big to follow", "too big to follow"));
    }
    if more > 0 {
        said.push(format!("{more} more not followed"));
    }
    let total = pieces.len() + more;
    let all = if total == 1 { "one piece".to_string() } else { format!("{total} pieces") };
    format!("{all}: {}", said.join(", "))
}

/// Counts something up by name.
fn tally<T: PartialEq>(counts: &mut Vec<(T, usize)>, name: T) {
    match counts.iter_mut().find(|(known, _)| *known == name) {
        Some((_, count)) => *count += 1,
        None => counts.push((name, 1)),
    }
}

/// "a spaceship", "3 spaceships"; "one that grows", "3 that grow".
fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 if one.starts_with("one ") => one.to_string(),
        1 => format!("{} {one}", if one.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" }),
        n => format!("{n} {many}"),
    }
}

/// What has left the small world so far, for a pattern that never repeats.
fn left_so_far(subject: &Subject) -> Option<String> {
    let census = subject.census.as_ref()?;
    let (ships, others) = (census.ships(), census.others());
    let mut kinds: Vec<&cas_core::census::Kind> = census.kinds().iter().collect();
    kinds.sort_by_key(|kind| std::cmp::Reverse(kind.count));
    let named: Vec<String> = kinds
        .iter()
        .take(KINDS_NAMED)
        .map(|kind| {
            let (dx, dy) = kind.motion.displacement;
            let name = format!("{} {}", speed(&kind.motion), arrow(dx, dy));
            if kind.count > 1 { format!("{name} ×{}", kind.count) } else { name }
        })
        .collect();
    let mut said = match ships {
        0 => "no spaceship has left it".to_string(),
        _ => format!("{} left it ({})", counted(ships as usize, "spaceship", "spaceships"), named.join(", ")),
    };
    if kinds.len() > KINDS_NAMED {
        said.push_str(&format!(", {}", counted(kinds.len() - KINDS_NAMED, "one more kind", "more kinds")));
    }
    if others > 0 {
        said.push_str(&format!(", {} that were none", others));
    }
    Some(format!("{said}, {} cells in it now", count(subject.world.population())))
}

/// Which way a displacement points.
fn arrow(dx: i32, dy: i32) -> &'static str {
    const ARROWS: [[&str; 3]; 3] = [["↖", "↑", "↗"], ["←", "·", "→"], ["↙", "↓", "↘"]];
    ARROWS[(dy.signum() + 1) as usize][(dx.signum() + 1) as usize]
}
