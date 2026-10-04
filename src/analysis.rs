//! Analysing a pattern: a panel that runs a pattern of the grid on its own and says what it
//! does.
//!
//! A pattern is chosen by dragging a band around it on the grid (after **Analyse**), sent
//! over from the spaceship list, or typed or pasted as text into the panel. It is followed on
//! the unbounded plane until it repeats or gets out of hand ([`Analyser::study`]), and shown
//! living in a small world of its own, a torus just big enough for it.

use std::sync::Arc;

use bevy::{
    clipboard::Clipboard,
    feathers::{
        constants::fonts,
        controls::{FeathersButton, FeathersScrollbar, FeathersTextInput},
        cursor::EntityCursor,
        palette,
        theme::{ThemeTextColor, ThemedText},
        tokens,
    },
    input_focus::InputFocus,
    picking::hover::Hovered,
    platform::time::Instant,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    text::{EditableText, FontSource, FontSourceTemplate, FontWeight, TextEdit, TextEditChange},
    ui_widgets::{Activate, ControlOrientation, ScrollArea},
    window::SystemCursorIcon,
};

use cas_core::{
    census::Census,
    collection::Sort,
    pattern::{
        Analyser, Cell, Fate, GROWING, Heading, Motion, Piece, PieceKind, SPREADING, Study, Symmetry, Turn, Watch,
        from_rle, to_rle, way,
    },
    rules::BlockRule,
    universe::Universe,
};

use crate::{
    actions::KeyboardOwner,
    kept::Collected,
    kit::{
        ALIVE, AXES, Aspect, CELLS_COLUMN, COLUMN_GAP, DEAD, GLYPH, ORBIT, PERIOD_COLUMN, caption, dial, field_frame,
        group_digits, heading as column_title, icon_button, icons, mono, number, panel_title, picture, side_panel,
        tile, tile_label, tile_value,
    },
    sim::{Settings, SimSystems},
    view::{Framing, GridMaterial, GridParams, Stamp, blank_image, cell_image, edge_of, upload},
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
/// How far a pattern is followed, whatever it is: for so many generations, and a small one
/// for more, as long as following it is no more work than this, in cells times generations,
/// and for no more than so many. There are oscillators and spaceships of a handful of cells
/// that take millions of generations to be back. Or until it has so many times the cells it
/// set out with, or is so many times as wide, which for a small pattern is so many cells and
/// so wide at least.
const GENERATIONS: u32 = 16 * 8192;
const WORK: u64 = 1 << 28;
const MOST_GENERATIONS: u32 = 1 << 26;
const MOST_CELLS: (usize, usize) = (4, 1024);
const WIDEST: (i32, i32) = (2, 512);
/// A study is made on another thread, and most are done before the next frame. One that is
/// still on its way after so many seconds shows in the panel, with how far it has got.
const SHOWS_AFTER: f32 = 0.15;
/// The small world's clock makes up for no more than so many seconds of a frame that took
/// long: a window that was out of sight for an hour does not run an hour of generations.
const LONGEST_FRAME: f32 = 0.25;
/// So many characters of the pattern's text are put in its field, and so many in the note of
/// what was copied; Copy copies all of it.
const TEXT_SHOWN: usize = 16_384;
const NOTE_SHOWN: usize = 36;
/// The size of what the findings say, the width of their names, and the gap after those.
const VALUE_SIZE: f32 = 12.0;
const NAME_COLUMN: f32 = 66.0;
const NAME_GAP: f32 = 10.0;
/// So many kinds of spaceship, and of oscillator, are listed among the pieces.
const KINDS_LISTED: usize = 8;
/// The side of the box a sort of piece has its sign in, and the width of the column that
/// says how many pieces are of a kind.
const SORT_GLYPH: f32 = 36.0;
const COUNT_COLUMN: f32 = 40.0;
/// The findings are in tiles, two to a row: a tile is so wide at least, and takes a row to
/// itself when it has much to say.
const TILE: f32 = 170.0;

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
    /// The pieces are listed under their line, kind by kind and with their pictures. Folded
    /// away, a line to each sort of piece says what there is.
    listing: bool,
    /// The text of the pattern's field as it was last acted on, and what is wrong with it if
    /// it does not spell a pattern.
    typed: String,
    mistyped: Option<String>,
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
        analyser.max_generations = patience(cells.len());
        analyser.max_cells = (MOST_CELLS.0 * cells.len()).max(MOST_CELLS.1);
        analyser.max_extent = (WIDEST.0 * width.max(height)).max(WIDEST.1);
        analyser.watch = Some(watch);
        let study = analyser.study(cells, phase)?;
        let pace = match study.period {
            Some(period) => (period as f32 / PERIOD_SECONDS).clamp(PACE.0, PACE.1),
            None => OPEN_PACE,
        };
        let mut world = small_world(&study, &rule);
        // A pattern that repeats is left to go round its torus. One that does not would fill
        // it: what it sends out leaves through an open border instead, and is counted.
        let census = (study.period.is_none()).then(|| {
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

/// For how many generations a pattern of so many cells is followed.
fn patience(cells: usize) -> u32 {
    let affordable = WORK / cells.max(1) as u64;
    affordable.clamp(GENERATIONS as u64, MOST_GENERATIONS as u64) as u32
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
    Cells,
    Changes,
    Size,
    Symmetry,
    Pieces,
}

/// The tile of a finding, which is not there while the finding has nothing to say; the box in
/// it where the finding is drawn; and the second line of what it says.
#[derive(Component, Default, Clone, Copy)]
struct Tile(Finding);

#[derive(Component, Default, Clone, Copy)]
struct Drawn(Finding);

#[derive(Component, Default, Clone, Copy)]
struct More(Finding);

/// The field with the pattern as text: what is on display, or what is typed or pasted there
/// to be studied.
#[derive(Component, Default, Clone)]
struct PatternText;

/// The line under the small world's caption: its generation, and what has left it.
#[derive(Component, Default, Clone)]
struct SmallStatus;

/// The caption of the Pause button, which says Play while the small world stands still.
#[derive(Component, Default, Clone)]
struct PauseLabel;

/// The line under the buttons.
#[derive(Component, Default, Clone)]
struct Note;

/// When a part of the panel is there. The panel shows what there is to show: before any
/// pattern was studied, what it is for; then a pattern, or the study of one on its way.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
enum During {
    #[default]
    Nothing,
    Pattern,
    Study,
    PatternOrStudy,
}

impl During {
    /// Is this the time, with a pattern on display or a study showing?
    fn is_now(self, pattern: bool, study: bool) -> bool {
        match self {
            During::Nothing => !pattern && !study,
            During::Pattern => pattern,
            During::Study => study,
            During::PatternOrStudy => pattern || study,
        }
    }
}

/// The line of the pieces, which is not there while there are none to speak of.
#[derive(Component, Default, Clone)]
struct Row;

/// The button that keeps the pattern on display, and its label, which says whether it is
/// kept; the button that keeps all the kinds of piece; and the mark on a row of the list that
/// keeps its kind: which row.
#[derive(Component, Default, Clone)]
struct KeepButton;

#[derive(Component, Default, Clone)]
struct KeepLabel;

#[derive(Component, Default, Clone)]
struct KeepAll;

#[derive(Component, Default, Clone, Copy)]
struct KeepPiece(usize);

/// Where the pieces of a pattern are summed up, or listed kind by kind, and the mark on their
/// line that turns as the list opens.
#[derive(Component, Default, Clone)]
struct PiecesList;

#[derive(Component, Default, Clone)]
struct PiecesChevron;

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
        // Nothing is ever put down in the small world: its material has the picture of no stamp.
        let no_stamp = world.resource_mut::<Assets<Image>>().add(blank_image());
        let material = world.resource_mut::<Assets<GridMaterial>>().add(GridMaterial::new(image.clone(), no_stamp));
        Self { image, material, empty }
    }
}

pub struct AnalysisPlugin;

impl Plugin for AnalysisPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Analysis>()
            .init_resource::<SmallAssets>()
            .add_observer(attach_material)
            .add_observer(text_in_mono)
            .add_systems(Update, call_off.in_set(SimSystems::Input))
            .add_systems(
                Update,
                (
                    take_study,
                    run_small_world,
                    draw_small_world,
                    sync_text,
                    sync_panel,
                    sync_keep,
                    sync_facts,
                    list_pieces,
                    show_status,
                    label_pause,
                )
                    .chain()
                    .in_set(SimSystems::Present),
            );
    }
}

fn attach_material(add: On<Add, SmallView>, assets: Res<SmallAssets>, mut commands: Commands) {
    commands.entity(add.entity).insert(MaterialNode(assets.material.clone()));
}

/// A pattern's text is set in the fixed-width face, as text to copy is. The text input's own
/// scene already sets a `TextFont`, which a second one in ours would duplicate, so it is
/// replaced here.
fn text_in_mono(add: On<Add, PatternText>, assets: Res<AssetServer>, mut commands: Commands) {
    commands.entity(add.entity).insert(TextFont {
        font: FontSource::Handle(assets.load(fonts::MONO)),
        font_size: FontSize::Px(VALUE_SIZE),
        ..default()
    });
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
                        (
                            // What the panel is for, until there is a pattern in it.
                            #AnalysisIntro
                            caption("A pattern on its own, followed until it repeats or gets out of hand, and left to live in a small world. Choose one with Analyse and a drag over the grid, send a spaceship over from the list, or type or paste a pattern's text here.")
                            template_value(During::Nothing)
                        ),
                        (
                            #AnalysisView
                            Node {
                                display: Display::None,
                                width: px(VIEW.0),
                                height: px(VIEW.1),
                                flex_shrink: 0.0,
                            }
                            SmallView
                            template_value(During::Pattern)
                        ),
                        (
                            Node {
                                display: Display::None,
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(6),
                            }
                            template_value(During::PatternOrStudy)
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
                                    template_value(During::Pattern)
                                    on(pause_world)
                                ),
                                (
                                    #AnalysisRestart
                                    @FeathersButton {
                                        @caption: bsn! { Text("Restart") ThemedText }
                                    }
                                    Node { flex_shrink: 0.0, min_height: px(22), padding: UiRect::axes(px(8), px(0)) }
                                    template_value(During::Pattern)
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
                            Node { display: Display::None }
                            template_value(During::PatternOrStudy)
                        ),
                        (
                            // What the study found, a tile to each finding, two to a row.
                            #AnalysisFacts
                            Node {
                                display: Display::None,
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(6),
                                row_gap: px(6),
                                flex_shrink: 0.0,
                            }
                            template_value(During::Pattern)
                            Children [
                                fact_tile("WHAT", Finding::What),
                                fact_tile("PERIOD", Finding::Period),
                                fact_tile("CELLS", Finding::Cells),
                                fact_tile("CHANGES", Finding::Changes),
                                fact_tile("SIZE", Finding::Size),
                                fact_tile("SYMMETRY", Finding::Symmetry),
                            ]
                        ),
                        (
                            // The pattern as text: to read, and to type or paste another into.
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(NAME_GAP),
                                flex_shrink: 0.0,
                            }
                            Children [
                                (column_title("TEXT") Node { width: px(NAME_COLUMN), flex_shrink: 0.0 }),
                                (
                                    field_frame()
                                    Node { flex_basis: px(0), min_width: px(0) }
                                    Children [(
                                        #StudyText
                                        @FeathersTextInput {}
                                        PatternText
                                        on(text_edited)
                                    )]
                                ),
                            ]
                        ),
                        (
                            Node {
                                display: Display::None,
                                flex_direction: FlexDirection::Row,
                                column_gap: px(6),
                            }
                            template_value(During::Pattern)
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
                                (
                                    // There for what comes back to its shape, as one thing.
                                    #AnalysisKeep
                                    @FeathersButton {
                                        @caption: bsn! { Text("Keep") ThemedText KeepLabel }
                                    }
                                    Node { display: Display::None, flex_grow: 1.0 }
                                    KeepButton
                                    on(keep_subject)
                                ),
                            ]
                        ),
                        (#AnalysisNote caption("") Note),
                        (
                            // The pieces come last: their list may be long, and what is above
                            // it, the buttons too, stays where it is when the list opens.
                            Node {
                                display: Display::None,
                                flex_direction: FlexDirection::Column,
                                row_gap: px(5),
                                flex_shrink: 0.0,
                            }
                            template_value(During::Pattern)
                            Children [
                                pieces_line(),
                                (
                                    #PiecesList
                                    Node {
                                        display: Display::None,
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(4),
                                    }
                                    PiecesList
                                ),
                            ]
                        ),
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

/// A finding in a tile, as the rule editor shows a property of a rule: a picture of it, its
/// name, and what was found in a line or two.
fn fact_tile(label: &'static str, finding: Finding) -> impl Scene {
    let name = Name::new(format!("Study{finding:?}"));
    let (this, drawn, more) = (Tile(finding), Drawn(finding), More(finding));
    bsn! {
        tile()
        Node {
            display: Display::None,
            flex_grow: 1.0,
            flex_basis: px(TILE),
            min_width: px(0),
        }
        template_value(name)
        template_value(this)
        Children [
            (
                Node {
                    width: px(GLYPH),
                    height: px(GLYPH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: px(4),
                }
                BackgroundColor(DEAD)
                template_value(drawn)
            ),
            (
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_width: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                }
                Children [
                    tile_label(label),
                    (tile_value("") template_value(finding)),
                    (
                        tile_value("")
                        TextColor(palette::LIGHT_GRAY_2)
                        Node { display: Display::None }
                        template_value(more)
                    ),
                ]
            ),
        ]
    }
}

/// The line of the pieces: its name, with the mark that turns as the list under it opens,
/// how many pieces there are, and a button that keeps every kind of them. A click on the line
/// opens the list or folds it away.
fn pieces_line() -> impl Scene {
    let name = Name::new("StudyPieces");
    bsn! {
        #PiecesToggle
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(NAME_GAP),
            min_height: px(26),
        }
        Row
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        on(|_: On<Pointer<Click>>, mut analysis: ResMut<Analysis>| analysis.listing = !analysis.listing)
        Children [
            (
                Node {
                    width: px(NAME_COLUMN),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(5),
                }
                template_value(Pickable::IGNORE)
                Children [
                    (
                        Text("PIECES")
                        TextFont {
                            font: FontSourceTemplate::Handle(fonts::BOLD),
                            font_size: FontSize::Px(10.0),
                            weight: FontWeight::BOLD,
                        }
                        ThemeTextColor(tokens::TEXT_DIM)
                        template_value(Pickable::IGNORE)
                    ),
                    (
                        icons::icon(icons::OPENS, 10.0, palette::LIGHT_GRAY_2)
                        UiTransform
                        PiecesChevron
                        template_value(Pickable::IGNORE)
                    ),
                ]
            ),
            (
                Text("—")
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                    font_size: FontSize::Px(VALUE_SIZE),
                    weight: FontWeight::NORMAL,
                }
                ThemeTextColor(tokens::TEXT_MAIN)
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                }
                template_value(name)
                template_value(Finding::Pieces)
                template_value(Pickable::IGNORE)
            ),
            (
                // Keeps every kind of piece that comes back to its shape, list open or not.
                #PiecesKeep
                @FeathersButton {
                    @caption: bsn! { Text("Keep all") ThemedText }
                }
                Node { display: Display::None, flex_shrink: 0.0 }
                KeepAll
                on(keep_pieces)
            ),
        ]
    }
}

/// A sort of piece in a tile, as the findings are: its sign, its name, how many pieces are of
/// it, and in dimmer letters the kinds among them. A sort with kinds to name has a row of the
/// panel to itself.
fn sort_tile(index: usize, sort: &Summary) -> impl Scene {
    let name = Name::new(format!("PieceSort{index}"));
    let basis = if sort.kinds.is_empty() { px(TILE) } else { percent(100) };
    // The arrows among the kinds are set in the face that has them all.
    let kinds: Vec<_> = runs(&sort.kinds)
        .into_iter()
        .map(|(run, arrows)| {
            let font = if arrows { fonts::MONO } else { fonts::REGULAR };
            bsn! {
                TextSpan(run)
                TextFont {
                    font: FontSourceTemplate::Handle(font),
                    font_size: FontSize::Px(VALUE_SIZE),
                    weight: FontWeight::NORMAL,
                }
                TextColor(palette::LIGHT_GRAY_2)
            }
        })
        .collect();
    let named = if sort.kinds.is_empty() { Display::None } else { Display::Flex };
    bsn! {
        tile()
        Node {
            flex_grow: 1.0,
            flex_basis: basis,
            min_width: px(0),
        }
        template_value(name)
        Children [
            (
                Node {
                    width: px(SORT_GLYPH),
                    height: px(SORT_GLYPH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: px(4),
                }
                BackgroundColor(DEAD)
                Children [ icons::icon(sort.icon, 18.0, sort.ink) ]
            ),
            (
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_width: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                }
                Children [
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Baseline,
                            column_gap: px(6),
                        }
                        Children [
                            (
                                Text(count(sort.count))
                                TextFont {
                                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                                    font_size: FontSize::Px(VALUE_SIZE),
                                    weight: FontWeight::NORMAL,
                                }
                                TextColor(palette::WHITE)
                            ),
                            tile_label(sort.label),
                        ]
                    ),
                    (
                        // In a line of its own, under how many there are.
                        Node { display: named }
                        Children [(
                            Text("")
                            TextFont {
                                font: FontSourceTemplate::Handle(fonts::REGULAR),
                                font_size: FontSize::Px(VALUE_SIZE),
                                weight: FontWeight::NORMAL,
                            }
                            TextColor(palette::LIGHT_GRAY_2)
                            Children [ {kinds} ]
                        )]
                    ),
                ]
            ),
        ]
    }
}

/// The name of a section of the list of pieces, over the columns of its rows. What keeps
/// still has no period to speak of.
fn pieces_heading(title: &'static str, period: bool) -> impl Scene {
    let period = if period { "PERIOD" } else { "" };
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(COLUMN_GAP),
            padding: UiRect { left: px(7), right: px(7), top: px(3) },
        }
        Children [
            (Node { flex_grow: 1.0, flex_basis: px(0) } Children [ column_title(title) ]),
            (Node { width: px(PERIOD_COLUMN), justify_content: JustifyContent::End } Children [ column_title(period) ]),
            (Node { width: px(CELLS_COLUMN), justify_content: JustifyContent::End } Children [ column_title("CELLS") ]),
            (Node { width: px(COUNT_COLUMN), justify_content: JustifyContent::End } Children [ column_title("COUNT") ]),
            (Node { width: px(24) }),
        ]
    }
}

/// One kind of piece, as the spaceship list shows a kind of spaceship: its picture, how it
/// moves or how large it is, the dial of the ways its ships fly, its period, its cells, how
/// many of it there are, the mark that keeps the kind, lit if it is kept, and as a bar the
/// share of its pieces in the `of` pieces of its section. The picture of a kind is the form
/// it is filed under, whichever way its pieces lie.
fn piece_row(index: usize, kind: &Listed, of: usize, kept: bool) -> impl Scene {
    let name = Name::new(format!("Piece{index}"));
    let (keep, keep_name) = (KeepPiece(index), Name::new(format!("PieceKeep{index}")));
    let ink = if kept { Aspect::Pattern.color() } else { palette::LIGHT_GRAY_2 };
    let share = percent(100.0 * kind.count as f32 / of.max(1) as f32);
    let bar = Aspect::Pattern.color();
    let title = mono(kind.title.clone(), 14.0, palette::WHITE);
    let about: Box<dyn SceneList> =
        if kind.note.is_empty() { bsn_list![title].into() } else { bsn_list![title, caption(kind.note)].into() };
    let period = kind.period.map_or(String::new(), |period| period.to_string());
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::axes(px(7), px(5)),
            border_radius: px(5),
            flex_shrink: 0.0,
        }
        BackgroundColor(palette::GRAY_2)
        template_value(name)
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(COLUMN_GAP),
                }
                Children [
                    picture(&kind.form),
                    (
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_width: px(0),
                            overflow: Overflow::clip(),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                        }
                        Children [ { about } ]
                    ),
                    dial(&kind.ways, None),
                    number(period, PERIOD_COLUMN, palette::LIGHT_GRAY_1),
                    number(kind.cells.to_string(), CELLS_COLUMN, palette::LIGHT_GRAY_1),
                    number(group_digits(kind.count as i64), COUNT_COLUMN, palette::WHITE),
                    (
                        icon_button(icons::KEEP, ink)
                        template_value(keep_name)
                        template_value(keep)
                        on(keep_piece)
                    ),
                ]
            ),
            (
                Node {
                    height: px(3),
                    border_radius: BorderRadius::MAX,
                }
                BackgroundColor(palette::GRAY_0)
                Children [(
                    Node {
                        width: share,
                        height: percent(100),
                        border_radius: BorderRadius::MAX,
                    }
                    BackgroundColor(bar)
                )]
            ),
        ]
    }
}

/// What of the pattern on display there is to keep: what it is, and the motion it is filed
/// under. Only what comes back to its shape as one thing: of a pattern of many pieces the
/// pieces are kept.
fn keepable(subject: &Subject) -> Option<(Sort, &Motion)> {
    let study = &subject.study;
    let motion = study.motion.as_ref().filter(|_| study.parts <= 1)?;
    Some((Sort::of(motion.displacement, study.still), motion))
}

/// Keeps the pattern on display among the patterns of its rule, or lets go of it if it is
/// kept.
fn keep_subject(_: On<Activate>, mut analysis: ResMut<Analysis>, mut collected: ResMut<Collected>) {
    let Some(subject) = analysis.shown() else {
        return;
    };
    let Some((sort, motion)) = keepable(subject) else {
        return;
    };
    let kept = collected.keep_or_forget(&subject.rule, sort, &motion.canonical, motion.period, motion.displacement);
    let sorts = sort.names();
    analysis.note = Some(match kept {
        true => format!("Kept among the {sorts} of its rule."),
        false => format!("Let go of: it is no longer among the {sorts} kept."),
    });
}

/// The kinds of piece that the list has a row for, in the order of the rows: of each sort so
/// many at most.
fn rows_of(kinds: &[Vec<Listed>; 3]) -> Vec<(Sort, &Listed)> {
    let sorted = Sort::ALL.into_iter().zip(kinds);
    sorted.flat_map(|(sort, kinds)| kinds.iter().take(KINDS_LISTED).map(move |kind| (sort, kind))).collect()
}

/// Keeps every kind of piece of the pattern on display that comes back to its shape: the
/// spaceships, the oscillators and the still lifes among them. The click goes no further:
/// the line would open its list.
fn keep_pieces(mut click: On<Pointer<Click>>, mut analysis: ResMut<Analysis>, mut collected: ResMut<Collected>) {
    click.propagate(false);
    if click.button != PointerButton::Primary {
        return;
    }
    let Some(subject) = analysis.shown() else {
        return;
    };
    let kinds = listed(&subject.study.pieces);
    let (mut new, mut known) = (0, 0);
    for (sort, kinds) in Sort::ALL.into_iter().zip(&kinds) {
        for kind in kinds {
            match collected.keep(&subject.rule, sort, &kind.form, kind.period.unwrap_or(1), kind.moves) {
                true => new += 1,
                false => known += 1,
            }
        }
    }
    let kinds = match new {
        1 => "one kind of piece".to_string(),
        new => format!("{new} kinds of piece"),
    };
    let were = if known == 1 { "was" } else { "were" };
    analysis.note = Some(match (new, known) {
        (0, _) => "Every kind of them was kept already.".to_string(),
        (_, 0) => format!("Kept {kinds}."),
        _ => format!("Kept {kinds}, and {known} {were} kept already."),
    });
}

/// The mark of a row keeps its kind of piece, or lets go of it.
fn keep_piece(
    mut click: On<Pointer<Click>>,
    marks: Query<&KeepPiece>,
    analysis: Res<Analysis>,
    mut collected: ResMut<Collected>,
) {
    let Ok(&KeepPiece(index)) = marks.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if click.button != PointerButton::Primary {
        return;
    }
    let Some(subject) = analysis.shown() else {
        return;
    };
    let kinds = listed(&subject.study.pieces);
    if let Some(&(sort, kind)) = rows_of(&kinds).get(index) {
        collected.keep_or_forget(&subject.rule, sort, &kind.form, kind.period.unwrap_or(1), kind.moves);
    }
}

/// Shows the button that keeps the pattern while there is something to keep, and has it say
/// whether that is kept.
fn sync_keep(
    analysis: Res<Analysis>,
    collected: Res<Collected>,
    mut button: Single<&mut Node, With<KeepButton>>,
    mut label: Single<&mut Text, With<KeepLabel>>,
) {
    let thing = analysis.shown().and_then(|subject| Some((subject, keepable(subject)?.1)));
    let display = if thing.is_some() { Display::Flex } else { Display::None };
    if button.display != display {
        button.display = display;
    }
    let kept = thing.is_some_and(|(subject, motion)| collected.is_kept(&subject.rule, &motion.canonical));
    label.set_if_neq(Text(if kept { "Kept" } else { "Keep" }.to_string()));
}

/// The pattern goes back on the grid: picked up, to be put down wherever. Under another rule
/// than it was studied in, the same cells go down, filed for that rule's vacuum.
fn place_subject(_: On<Activate>, universe: Res<Universe>, mut analysis: ResMut<Analysis>, mut stamp: ResMut<Stamp>) {
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
        Ok(()) => format!("Copied {}", head(&rle, NOTE_SHOWN)),
        Err(error) => format!("The clipboard is not available ({error:?})."),
    });
}

/// Typing or pasting in the text field studies the pattern the text spells, as soon as it
/// spells one, under the rule of the grid. The text is the one Copy gives: the pattern from a
/// corner of the blocks the next step rewrites, at the start of the vacuum's cycle.
fn text_edited(
    _: On<TextEditChange>,
    field: Single<(Entity, &EditableText), With<PatternText>>,
    focus: Res<InputFocus>,
    universe: Res<Universe>,
    mut analysis: ResMut<Analysis>,
) {
    let (entity, text) = *field;
    // The field is also rewritten for every pattern studied; only the user's edits count.
    if focus.get() != Some(entity) {
        return;
    }
    // Text from elsewhere may have been broken into lines.
    let typed: String = text.value().to_string().split_whitespace().collect();
    // Moving the caret reports an edit as well, so most of the time nothing is new.
    if typed == analysis.bypass_change_detection().typed {
        return;
    }
    analysis.typed = typed;
    match from_rle(&analysis.typed) {
        // An emptied field is on its way to another pattern.
        Ok(cells) if cells.is_empty() => analysis.mistyped = None,
        Ok(cells) => {
            analysis.mistyped = None;
            analysis.study(cells, 0, &universe);
        }
        Err(error) => analysis.mistyped = Some(format!("{error}.")),
    }
}

/// Puts the text of the pattern on display in its field, unless the user is typing there: for
/// a new pattern, and when the field is left with something else in it.
fn sync_text(
    mut analysis: ResMut<Analysis>,
    focus: Res<InputFocus>,
    mut field: Single<(Entity, &mut EditableText), With<PatternText>>,
    mut shown: Local<Option<u64>>,
) {
    let (entity, text) = &mut *field;
    if focus.get() == Some(*entity) {
        return;
    }
    let number = analysis.subject.as_ref().map(|subject| subject.number);
    if *shown == number && !focus.is_changed() {
        return;
    }
    *shown = number;
    let wanted = analysis.subject.as_ref().map_or(String::new(), |subject| head(&subject.rle, TEXT_SHOWN));
    if text.value().to_string() != wanted {
        text.queue_edit(TextEdit::SelectAll);
        text.queue_edit(TextEdit::Insert(wanted.as_str().into()));
    }
    // A long text shows from its beginning: the field keeps its caret in sight.
    text.queue_edit(TextEdit::TextStart(false));
    if analysis.typed != wanted || analysis.mistyped.is_some() {
        analysis.typed = wanted;
        analysis.mistyped = None;
    }
}

/// So much of a text as there is room for, and a mark where the rest was left out.
fn head(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
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

/// Escape calls the choosing off, unless it is pressed to leave a text field.
fn call_off(keys: Res<ButtonInput<KeyCode>>, typing: KeyboardOwner, mut analysis: ResMut<Analysis>) {
    if keys.just_pressed(KeyCode::Escape) && !typing.is_some() && (analysis.selecting || analysis.band.is_some()) {
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
    panel: Single<Entity, With<AnalysisPanel>>,
    parts: Query<(Entity, &During)>,
    mut nodes: Query<&mut Node>,
    mut captions: Query<&mut Text, (With<SmallCaption>, Without<Note>, Without<SelectHint>)>,
    mut rule_name: Single<&mut Text, (With<SubjectRule>, Without<SmallCaption>, Without<Note>, Without<SelectHint>)>,
    mut note: Single<(Entity, &mut Text), (With<Note>, Without<SelectHint>)>,
    mut hint: Single<&mut Text, (With<SelectHint>, Without<Note>)>,
    mut mark: Single<&mut BorderColor, With<ChoosingMark>>,
    mut shown: Local<Option<Shown>>,
) {
    let mut show = |entity: Entity, shown: bool| {
        let display = if shown { Display::Flex } else { Display::None };
        if let Ok(mut node) = nodes.get_mut(entity)
            && node.display != display
        {
            node.display = display;
        }
    };
    show(*panel, analysis.open);
    let busy = analysis.busy();
    let now = Shown {
        subject: analysis.subject.as_ref().map(|subject| subject.number),
        busy: busy.map(|_| analysis.studied),
        selecting: analysis.selecting,
        // A stamp from the list is the list's business.
        holding: stamp.is_held() && stamp.kind.is_none(),
        // What is wrong with the text being typed comes before what a button did.
        note: analysis.mistyped.clone().or(analysis.note.clone()),
    };
    if shown.as_ref() == Some(&now) {
        return;
    }
    let now = shown.insert(now);

    // What there is to show: a pattern, the study of one on its way, or neither yet.
    let subject = analysis.shown();
    for (part, during) in &parts {
        show(part, during.is_now(subject.is_some(), busy.is_some()));
    }
    for mut text in &mut captions {
        let content = match (subject, busy) {
            (Some(subject), _) => {
                let (width, height) = (subject.world.width, subject.world.height);
                format!("{width}×{height} cells, {} generations a second", subject.pace.round())
            }
            (None, Some(studying)) => {
                let cells = if studying.cells == 1 { "cell" } else { "cells" };
                format!("{} {cells}, for {} at most", count(studying.cells), generations(patience(studying.cells)))
            }
            _ => String::new(),
        };
        text.set_if_neq(Text(content));
    }
    let rule = subject.map(|subject| &subject.rule).or(busy.map(|studying| &studying.rule));
    rule_name.set_if_neq(Text(rule.map_or("", |rule| rule.name()).to_string()));
    let what_next = match (analysis.selecting, subject.is_some(), now.holding) {
        _ if busy.is_some() => {
            "Stop takes what is known by then for the study. A drag over another pattern studies that one instead."
        }
        (_, _, true) => {
            "Click the grid to put the pattern down, as often as you like. Escape or a right click lets go of it."
        }
        (true, false, _) => "Drag over the pattern on the grid. Escape calls it off.",
        (true, true, _) => {
            "Drag over another pattern, or Place picks this one up to be put down on the grid where you click."
        }
        // Before any pattern, the panel itself says what to do.
        (false, false, _) => "",
        (false, true, _) => "Place picks the pattern up, to be put down on the grid where you click.",
    };
    let (line, text) = &mut *note;
    let said = now.note.clone().unwrap_or(what_next.to_string());
    show(*line, !said.is_empty());
    text.set_if_neq(Text(said));
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
            if studying.watch.back() {
                // Not to be stopped: it takes as long again, and then all is known.
                format!("{generation}: it is back, and is gone through once more")
            } else if studying.watch.stopped() {
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

/// Keeps the findings in step with the pattern on display: what each says, and the picture of
/// it. A tile with nothing to say is not there, and neither is the line of the pieces.
fn sync_facts(
    analysis: Res<Analysis>,
    tiles: Query<(Entity, &Tile)>,
    pictures: Query<(Entity, &Drawn)>,
    mut values: Query<(Entity, &Finding, &mut Text, &TextColor, &ChildOf), Without<More>>,
    mut mores: Query<(Entity, &More, &mut Text, &TextColor), Without<Finding>>,
    rows: Query<(), With<Row>>,
    mut nodes: Query<&mut Node>,
    assets: Res<AssetServer>,
    mut shown: Local<Option<Option<u64>>>,
    mut commands: Commands,
) {
    let subject = analysis.shown();
    let now = subject.map(|subject| subject.number);
    if shown.replace(now) == Some(now) {
        return;
    }
    let said = |finding: Finding| subject.and_then(|subject| fact(finding, subject));
    let mut show = |entity: Entity, shown: bool, basis: Option<Val>| {
        let display = if shown { Display::Flex } else { Display::None };
        if let Ok(mut node) = nodes.get_mut(entity) {
            if node.display != display {
                node.display = display;
            }
            if let Some(basis) = basis.filter(|&basis| node.flex_basis != basis) {
                node.flex_basis = basis;
            }
        }
    };
    // The arrows in a line are set in the face that has them all: what comes after the first
    // run of text goes into spans of its own.
    let mut write = |line: Entity, text: &mut Mut<Text>, color: TextColor, content: &str| {
        let mut runs = runs(content);
        let first = if runs.first().is_some_and(|(_, arrows)| !arrows) { runs.remove(0).0 } else { String::new() };
        text.set_if_neq(Text(first));
        commands.entity(line).despawn_related::<Children>();
        for (run, arrows) in runs {
            let font = FontSource::Handle(assets.load(if arrows { fonts::MONO } else { fonts::REGULAR }));
            let font = TextFont { font, font_size: FontSize::Px(VALUE_SIZE), ..default() };
            commands.entity(line).with_child((TextSpan::new(run), font, color));
        }
    };
    for (tile, &Tile(finding)) in &tiles {
        let fact = said(finding);
        // A tile with much to say has a row to itself.
        let basis = if fact.as_ref().is_some_and(|fact| fact.wide) { percent(100) } else { px(TILE) };
        show(tile, fact.is_some(), Some(basis));
    }
    for (line, &finding, mut text, color, parent) in &mut values {
        let fact = said(finding);
        if rows.contains(parent.parent()) {
            show(parent.parent(), fact.is_some(), None);
        }
        write(line, &mut text, *color, fact.as_ref().map_or("", |fact| &fact.value));
    }
    for (line, &More(finding), mut text, color) in &mut mores {
        let more = said(finding).map_or(String::new(), |fact| fact.more);
        show(line, !more.is_empty(), None);
        write(line, &mut text, *color, &more);
    }
    for (frame, &Drawn(finding)) in &pictures {
        commands.entity(frame).despawn_related::<Children>();
        if let Some(subject) = subject {
            let picture = draw(finding, subject, &mut commands);
            commands.entity(frame).add_children(&picture);
        }
    }
}

/// What a finding says: in a word or two, with a line more where there is more to say; and
/// whether that is so much that its tile takes a row to itself.
#[derive(Debug, Default, PartialEq)]
struct Fact {
    value: String,
    more: String,
    wide: bool,
}

/// What the study says of one thing, in words; nothing where that does not apply.
fn fact(finding: Finding, subject: &Subject) -> Option<Fact> {
    let study = &subject.study;
    let motion = study.motion.as_ref();
    let travels = motion.is_some_and(|motion| motion.heading() != Heading::Still);
    let said = |value: String, more: String| Some(Fact { value, more, wide: false });
    // What does not come back has more to say about how far it got, and less besides.
    let wide = |value: &str, more: String| Some(Fact { value: value.to_string(), more, wide: true });
    match finding {
        Finding::What => match (study.still, motion, study.fate) {
            (true, ..) => said("Still life".to_string(), String::new()),
            // The motion is that of the form it is filed under; the way it goes is as it lies.
            (_, Some(motion), Fate::Returns { displacement: (dx, dy), .. }) if travels => {
                said("Spaceship".to_string(), format!("{} {} {}", speed(motion), heading(motion), arrow(dx, dy)))
            }
            // Followed until it was back, or known by its pieces to come back.
            _ if study.period.is_some() => match study.parts {
                0 | 1 => said("Oscillator".to_string(), String::new()),
                parts => said("Oscillator".to_string(), format!("of {} pieces that never meet", count(parts))),
            },
            // A gun's streams get too wide before they are too many cells: what grows while
            // it flies apart grows.
            (_, _, Fate::Grows | Fate::Scatters) if study.growth.is_some_and(|growth| growth >= GROWING) => {
                let how = if study.growth >= Some(SPREADING) { "over the plane" } else { "along lines, as a gun does" };
                let far = format!("{} cells after {}", count(study.cells.1), generations(study.generations));
                wide(&format!("Grows {how}"), far)
            }
            (_, _, Fate::Grows) => {
                wide("Grows", format!("{} cells after {}", count(study.cells.1), generations(study.generations)))
            }
            (_, _, Fate::Scatters) => {
                let across = study.extent.0.max(study.extent.1);
                wide("Flies apart", format!("{across} cells across after {}", generations(study.generations)))
            }
            _ => wide("Undecided", format!("after {}", generations(study.generations))),
        },
        Finding::Period => {
            let sooner = match study.recurs {
                Some((after, turn)) if !study.still => format!("{} after {after}", turned(turn)),
                _ => String::new(),
            };
            match (study.still, study.fate, study.period?) {
                (true, ..) => said(generations(1u32), String::new()),
                (false, Fate::Returns { displacement: (dx, dy), .. }, period) if travels => {
                    let moving = format!("moving ({dx}, {dy})");
                    said(generations(period), if sooner.is_empty() { moving } else { format!("{moving}, {sooner}") })
                }
                (false, Fate::Returns { .. }, period) => said(generations(period), sooner),
                // Longer than it was followed for: all its pieces have to be back at once.
                (false, _, period) => Some(Fact {
                    value: generations(period),
                    more: "until all its pieces are back at once".to_string(),
                    wide: true,
                }),
            }
        }
        Finding::Cells => match study.cells {
            (fewest, most) if fewest == most => said(count(fewest), String::new()),
            (fewest, most) => said(format!("{} to {}", count(fewest), count(most)), String::new()),
        },
        Finding::Changes => {
            // In a narrow tile the line breaks after the cells, not before the last word.
            let heat = format!("{:.1} cells a\u{a0}generation", study.heat);
            match study.period? {
                _ if study.still => said("none".to_string(), String::new()),
                _ if !travels && study.stator > 0 => said(heat, format!("{} never change", count(study.stator))),
                _ => said(heat, String::new()),
            }
        }
        Finding::Size => {
            let (width, height) = bounding_box(&study.start);
            let grown = (width, height) != study.extent;
            let most = if grown { format!("up to {}×{}", study.extent.0, study.extent.1) } else { String::new() };
            said(format!("{width}×{height}"), most)
        }
        Finding::Symmetry => {
            let (which, so) = match study.symmetry {
                Symmetry::None => ("none", ""),
                Symmetry::Mirror => ("a mirror", ""),
                Symmetry::DiagonalMirror => ("a mirror across a diagonal", ""),
                Symmetry::HalfTurn => ("a half turn", ""),
                Symmetry::TwoMirrors => ("mirrors both ways", "and so a half turn"),
                Symmetry::TwoDiagonalMirrors => ("mirrors across both diagonals", "and so a half turn"),
                Symmetry::QuarterTurn => ("a quarter turn", ""),
                Symmetry::All => ("every turn and mirror", ""),
            };
            said(which.to_string(), so.to_string())
        }
        // How many there are, where there are several; what they are is said underneath, sort
        // by sort or kind by kind.
        Finding::Pieces => match (study.period, study.parts, study.pieces.len() + study.more_pieces) {
            (Some(_), parts, _) if parts > 1 => said(format!("{} that never meet", count(parts)), String::new()),
            (None, _, total) if total > 1 => said(format!("{} pieces", count(total)), String::new()),
            _ => None,
        },
    }
}

/// The picture of a finding, to go into the box of its tile: a sign for what the pattern is,
/// which for a spaceship points the way it flies; signs for its period, its cells and what
/// changes; its size as it set out, within the most it got to; and its symmetry as the rule
/// editor draws a rule's, a point with its images and the axes of the mirrors.
fn draw(finding: Finding, subject: &Subject, commands: &mut Commands) -> Vec<Entity> {
    let study = &subject.study;
    let ink = Aspect::Pattern.color();
    let mut sign = |glyph: &'static str, color: Color, turned: f32| {
        let turned = UiTransform::from_rotation(Rot2::degrees(turned));
        commands.spawn_scene(bsn! { icons::icon(glyph, 26.0, color) template_value(turned) }).id()
    };
    match finding {
        Finding::What => {
            let travels = study.motion.as_ref().is_some_and(|motion| motion.heading() != Heading::Still);
            let (glyph, turned) = match study.fate {
                _ if study.still => (icons::STILL, 0.0),
                // Its nose is up as it comes, and it is turned an eighth at a time.
                Fate::Returns { displacement: (dx, dy), .. } if travels => {
                    (icons::SHIP, 45.0 * way(dx, dy).unwrap_or(0) as f32)
                }
                _ if study.period.is_some() => (icons::OSCILLATES, 0.0),
                Fate::Grows => (icons::GROWS, 0.0),
                Fate::Scatters if study.growth.is_some_and(|growth| growth >= GROWING) => (icons::GROWS, 0.0),
                Fate::Scatters => (icons::APART, 0.0),
                _ => (icons::UNDECIDED, 0.0),
            };
            vec![sign(glyph, ink, turned)]
        }
        Finding::Period => vec![sign(icons::PERIOD, palette::LIGHT_GRAY_2, 0.0)],
        Finding::Cells => vec![sign(icons::CELLS, palette::LIGHT_GRAY_2, 0.0)],
        Finding::Changes => vec![sign(icons::CHANGES, palette::LIGHT_GRAY_2, 0.0)],
        Finding::Size => {
            // To scale: the most it got to as an outline, and in it the pattern as it set out.
            let (width, height) = bounding_box(&study.start);
            let scale = (GLYPH - 12.0) / study.extent.0.max(study.extent.1).max(1) as f32;
            let mut frame = |(width, height): (i32, i32), filled: bool| {
                let (width, height) = ((width as f32 * scale).max(2.0), (height as f32 * scale).max(2.0));
                let (fill, line) = if filled { (ALIVE, ALIVE) } else { (Color::NONE, palette::LIGHT_GRAY_2) };
                let frame = bsn! {
                    Node {
                        position_type: PositionType::Absolute,
                        left: px((GLYPH - width) / 2.0),
                        top: px((GLYPH - height) / 2.0),
                        width: px(width),
                        height: px(height),
                        border: px(1),
                    }
                    BackgroundColor(fill)
                    BorderColor::all(line)
                };
                commands.spawn_scene(frame).id()
            };
            vec![frame(study.extent, false), frame((width, height), true)]
        }
        Finding::Symmetry => {
            let middle = GLYPH / 2.0;
            let mut parts = Vec::new();
            for (element, degrees) in AXES {
                if study.symmetries[element] {
                    let turned = UiTransform::from_rotation(Rot2::degrees(degrees));
                    let axis = bsn! {
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(2),
                            top: px(middle - 0.5),
                            width: px(GLYPH - 4.0),
                            height: px(1),
                        }
                        BackgroundColor(ink)
                        template_value(turned)
                    };
                    parts.push(commands.spawn_scene(axis).id());
                }
            }
            for (element, (x, y)) in ORBIT.into_iter().enumerate() {
                let color = if study.symmetries[element] { palette::WHITE } else { palette::GRAY_3 };
                let dot = bsn! {
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(middle + x - 2.5),
                        top: px(middle + y - 2.5),
                        width: px(5),
                        height: px(5),
                        border_radius: BorderRadius::MAX,
                    }
                    BackgroundColor(color)
                };
                parts.push(commands.spawn_scene(dot).id());
            }
            parts
        }
        Finding::Pieces => Vec::new(),
    }
}

/// A line taken apart into what is text and what is arrows, in order. The face of the panel
/// has no arrows along the diagonals, and the mono face has all eight.
fn runs(line: &str) -> Vec<(String, bool)> {
    let mut runs: Vec<(String, bool)> = Vec::new();
    for symbol in line.chars() {
        let arrow = ['↖', '↑', '↗', '←', '→', '↙', '↓', '↘'].contains(&symbol);
        match runs.last_mut() {
            Some((run, arrows)) if *arrows == arrow => run.push(symbol),
            _ => runs.push((symbol.to_string(), arrow)),
        }
    }
    runs
}

fn generations(n: impl Into<u128>) -> String {
    let n = n.into();
    format!("{} generation{}", group_digits(n), if n == 1 { "" } else { "s" })
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
pub(crate) fn speed(motion: &Motion) -> String {
    match motion.speed() {
        (1, 1) => "c".to_string(),
        (1, period) => format!("c/{period}"),
        (cells, period) => format!("{cells}c/{period}"),
    }
}

pub(crate) fn heading(motion: &Motion) -> &'static str {
    match motion.heading() {
        Heading::Orthogonal => "orthogonal",
        Heading::Diagonal => "diagonal",
        _ => "oblique",
    }
}

/// A sort of piece, as a tile of the summary has it: how many pieces are of it, what they are
/// called, their sign, and a word on the kinds among them.
#[derive(Debug, PartialEq)]
struct Summary {
    count: usize,
    label: &'static str,
    icon: &'static str,
    ink: Color,
    kinds: String,
}

/// What the pieces of a pattern are, sort by sort: the spaceships with their speeds, the
/// oscillators with their periods, the still lifes, and whatever else became of pieces.
fn sorts(pieces: &[Piece], more: usize) -> Vec<Summary> {
    let [ships, ..] = listed(pieces);
    let mut flying: Vec<(String, usize)> = Vec::new();
    for kind in &ships {
        tally(&mut flying, kind.title.clone(), kind.count);
    }
    let mut periods: Vec<(u32, usize)> = Vec::new();
    let mut still = 0;
    for piece in pieces {
        match piece.kind {
            PieceKind::Oscillator { period } => tally(&mut periods, period, 1),
            PieceKind::StillLife => still += 1,
            _ => {}
        }
    }
    // The commoner period first, and of two as common the shorter.
    periods.sort_by_key(|&(period, count)| (std::cmp::Reverse(count), period));
    let periods: Vec<(String, usize)> = periods.iter().map(|(period, count)| (period.to_string(), *count)).collect();
    let of = if periods.len() == 1 { "period" } else { "periods" };
    let so_many = |kinds: &[(String, usize)]| kinds.iter().map(|(_, count)| count).sum::<usize>();
    let ink = Aspect::Pattern.color();
    let came_back = [
        (so_many(&flying), "SPACESHIP", "SPACESHIPS", icons::SHIP, commonest(&flying, "one more kind", "more kinds")),
        (
            so_many(&periods),
            "OSCILLATOR",
            "OSCILLATORS",
            icons::OSCILLATES,
            format!("{of} {}", commonest(&periods, "one more", "more")),
        ),
        (still, "STILL LIFE", "STILL LIFES", icons::STILL, String::new()),
    ];
    let mut sorts = Vec::new();
    for (count, one, many, icon, kinds) in came_back {
        if count > 0 {
            sorts.push(Summary { count, label: if count == 1 { one } else { many }, icon, ink, kinds });
        }
    }
    let dim = palette::LIGHT_GRAY_2;
    let rest = [
        (PieceKind::Grows, "THAT GROWS", "THAT GROW", icons::GROWS),
        (PieceKind::Scatters, "THAT FLIES APART", "THAT FLY APART", icons::APART),
        (PieceKind::Undecided, "STILL CHANGING", "STILL CHANGING", icons::UNDECIDED),
        (PieceKind::Unexamined, "TOO BIG TO FOLLOW", "TOO BIG TO FOLLOW", icons::UNDECIDED),
    ];
    for (kind, one, many, icon) in rest {
        let count = pieces.iter().filter(|piece| piece.kind == kind).count();
        if count > 0 {
            let label = if count == 1 { one } else { many };
            sorts.push(Summary { count, label, icon, ink: dim, kinds: String::new() });
        }
    }
    if more > 0 {
        sorts.push(Summary {
            count: more,
            label: "MORE, NOT FOLLOWED",
            icon: icons::UNDECIDED,
            ink: dim,
            kinds: String::new(),
        });
    }
    sorts
}

/// The commonest of some kinds by name, with how many there are of each where it is more than
/// one, and how many kinds were left out. A line breaks between kinds: a name stays with its
/// number, and the words about the rest stay together.
fn commonest(kinds: &[(String, usize)], one_more: &str, more: &str) -> String {
    let mut kinds: Vec<&(String, usize)> = kinds.iter().collect();
    kinds.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let named: Vec<String> = kinds
        .iter()
        .take(KINDS_NAMED)
        .map(|(name, count)| if *count > 1 && kinds.len() > 1 { format!("{name}\u{a0}×{count}") } else { name.clone() })
        .collect();
    match kinds.len().saturating_sub(KINDS_NAMED) {
        0 => named.join(", "),
        left => {
            let rest = format!("and {}", counted(left, one_more, more)).replace(' ', "\u{a0}");
            format!("{} {rest}", named.join(", "))
        }
    }
}

/// The pieces that neither travel nor stay as they are, by what became of them, and those
/// that were not followed: how many, and what to call one of them and several.
fn others(pieces: &[Piece], more: usize) -> Vec<(usize, &'static str, &'static str)> {
    let so_many = |kind: PieceKind| pieces.iter().filter(|piece| piece.kind == kind).count();
    [
        (so_many(PieceKind::Grows), "one that grows", "that grow"),
        (so_many(PieceKind::Scatters), "one that flies apart", "that fly apart"),
        (so_many(PieceKind::Undecided), "one still changing", "still changing"),
        (so_many(PieceKind::Unexamined), "one too big to follow", "too big to follow"),
        (more, "one more not followed", "more not followed"),
    ]
    .into_iter()
    .filter(|&(count, ..)| count > 0)
    .collect()
}

/// A kind of piece as the list shows it: the form it is filed under; how it moves, or how
/// large it is; a word more; how many of its ships fly each way, clockwise from straight up;
/// its period, unless it keeps still; its cells; and how many pieces are of the kind.
struct Listed {
    form: Vec<Cell>,
    title: String,
    note: &'static str,
    ways: [u64; 8],
    period: Option<u32>,
    cells: usize,
    count: usize,
    /// How far the form moves in a period, which is what a kind is kept with.
    moves: (i32, i32),
}

/// The pieces that came back to their shape, kind by kind, the commonest first: the
/// spaceships, the oscillators and the still lifes, in the order of [`Sort::ALL`].
fn listed(pieces: &[Piece]) -> [Vec<Listed>; 3] {
    let mut sorted: [Vec<Listed>; 3] = Default::default();
    for piece in pieces.iter().filter(|piece| !piece.form.is_empty()) {
        let size = || {
            let (width, height) = bounding_box(&piece.form);
            format!("{width}×{height}")
        };
        let (sort, title, note, period, way) = match piece.kind {
            PieceKind::Spaceship { period, displacement: (dx, dy) } => {
                let motion = Motion { period, displacement: (dx, dy), canonical: Vec::new() };
                (0, speed(&motion), heading(&motion), Some(period), way(dx, dy))
            }
            PieceKind::Oscillator { period } => (1, size(), "", Some(period), None),
            PieceKind::StillLife => (2, size(), "", None, None),
            _ => continue,
        };
        let kinds = &mut sorted[sort];
        let known = match kinds.iter().position(|known| known.form == piece.form) {
            Some(known) => known,
            None => {
                let form = piece.form.clone();
                let (cells, moves) = (piece.cells, piece.moves);
                kinds.push(Listed { form, title, note, ways: [0; 8], period, cells, count: 0, moves });
                kinds.len() - 1
            }
        };
        kinds[known].count += 1;
        if let Some(way) = way {
            kinds[known].ways[way] += 1;
        }
    }
    for kinds in &mut sorted {
        kinds.sort_by_key(|kind| std::cmp::Reverse(kind.count));
    }
    sorted
}

/// Says what the pieces of the pattern on display are, under their line: sort by sort in a
/// tile each, or, with the list open, kind by kind with their pictures: the spaceships, the
/// oscillators, the still lifes, and a word on the rest.
fn list_pieces(
    analysis: Res<Analysis>,
    collected: Res<Collected>,
    list: Single<Entity, With<PiecesList>>,
    keep_all: Single<Entity, With<KeepAll>>,
    mut chevron: Single<(Entity, &mut UiTransform), With<PiecesChevron>>,
    mut nodes: Query<&mut Node>,
    mut shown: Local<Option<(Option<u64>, bool, u64)>>,
    mut commands: Commands,
) {
    let subject = analysis.shown();
    let now = (subject.map(|subject| subject.number), analysis.listing, collected.revision());
    if shown.replace(now) == Some(now) {
        return;
    }
    // One piece is the pattern itself, of which all is said above.
    let (pieces, more) = match subject.map(|subject| &subject.study) {
        Some(study) if study.pieces.len() + study.more_pieces > 1 => (&study.pieces[..], study.more_pieces),
        _ => (&[][..], 0),
    };
    let kinds = listed(pieces);
    let sorts = sorts(pieces, more);
    // The list has the pieces that came back to their shape: without any, there is none, and
    // nothing to keep.
    let any = kinds.iter().any(|kinds| !kinds.is_empty());
    let listing = any && analysis.listing;
    let mut show = |entity: Entity, shown: bool| {
        let display = if shown { Display::Flex } else { Display::None };
        if let Ok(mut node) = nodes.get_mut(entity)
            && node.display != display
        {
            node.display = display;
        }
    };
    // The mark is there while there is something to list, and points down while it is listed.
    let (mark, turned) = &mut *chevron;
    show(*mark, any);
    show(*keep_all, any);
    turned.rotation = if listing { Rot2::FRAC_PI_2 } else { Rot2::IDENTITY };
    show(*list, !sorts.is_empty());
    commands.entity(*list).despawn_related::<Children>();
    let mut rows = Vec::new();
    if !listing {
        // The sorts in tiles, two to a row where they have little to say.
        let tiles: Vec<_> = sorts.iter().enumerate().map(|(index, sort)| sort_tile(index, sort)).collect();
        let summary = bsn! {
            Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(6),
            }
            Children [ {tiles} ]
        };
        rows.push(commands.spawn_scene(summary).id());
        commands.entity(*list).add_children(&rows);
        return;
    }
    let rule = subject.map(|subject| &subject.rule);
    let is_kept = |kind: &Listed| rule.is_some_and(|rule| collected.is_kept(rule, &kind.form));
    let mut index = 0;
    for (title, kinds) in ["SPACESHIPS", "OSCILLATORS", "STILL LIFES"].into_iter().zip(&kinds) {
        if kinds.is_empty() {
            continue;
        }
        let of = kinds.iter().map(|kind| kind.count).sum();
        let period = kinds.iter().any(|kind| kind.period.is_some());
        rows.push(commands.spawn_scene(pieces_heading(title, period)).id());
        for kind in kinds.iter().take(KINDS_LISTED) {
            rows.push(commands.spawn_scene(piece_row(index, kind, of, is_kept(kind))).id());
            index += 1;
        }
        if kinds.len() > KINDS_LISTED {
            let more = counted(kinds.len() - KINDS_LISTED, "one more kind", "more kinds");
            rows.push(commands.spawn_scene(caption(format!("and {more}"))).id());
        }
    }
    let rest: Vec<String> = others(pieces, more).iter().map(|&(count, one, many)| counted(count, one, many)).collect();
    if !rest.is_empty() {
        rows.push(commands.spawn_scene(caption(format!("Besides: {}.", rest.join(", ")))).id());
    }
    commands.entity(*list).add_children(&rows);
}

/// Counts so many more of something up by name.
fn tally<T: PartialEq>(counts: &mut Vec<(T, usize)>, name: T, more: usize) {
    match counts.iter_mut().find(|(known, _)| *known == name) {
        Some((_, count)) => *count += more,
        None => counts.push((name, more)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_comes_apart_into_text_and_arrows() {
        let run = |text: &str, arrows: bool| (text.to_string(), arrows);
        assert_eq!(runs("c/6 orthogonal →"), [run("c/6 orthogonal ", false), run("→", true)]);
        assert_eq!(
            runs("2 spaceships (c/15 ↘, c/6 →)"),
            [run("2 spaceships (c/15 ", false), run("↘", true), run(", c/6 ", false), run("→", true), run(")", false)]
        );
        assert_eq!(runs("↖↗ both"), [run("↖↗", true), run(" both", false)]);
        assert_eq!(runs("a mirror"), [run("a mirror", false)]);
        assert!(runs("").is_empty());
    }

    #[test]
    fn a_small_pattern_is_followed_for_longer() {
        // As far as any pattern was, at the least: the many cells of what a blob leaves.
        assert_eq!((patience(100_000), patience(2048)), (GENERATIONS, GENERATIONS));
        // A hundred cells for millions of generations, a handful for tens of millions: the
        // spaceship of 7 328 092 generations and the oscillator of 32 782 820 have eight
        // cells at most.
        assert_eq!(patience(100), 2_684_354);
        assert!(patience(8) > 32_782_820);
        // And no pattern for longer than that.
        assert_eq!((patience(4), patience(1), patience(0)), (MOST_GENERATIONS, MOST_GENERATIONS, MOST_GENERATIONS));
    }

    #[test]
    fn pieces_are_listed_kind_by_kind() {
        let ship = vec![(1, 0), (2, 0), (1, 2), (2, 2)];
        let flying = |displacement| Piece {
            cells: 4,
            kind: PieceKind::Spaceship { period: 12, displacement },
            form: ship.clone(),
            moves: (2, 0),
        };
        let apart = [
            flying((2, 0)),
            Piece { cells: 1, kind: PieceKind::Oscillator { period: 4 }, form: vec![(0, 0)], moves: (0, 0) },
            flying((-2, 0)),
            Piece { cells: 4, kind: PieceKind::StillLife, form: vec![(0, 1), (1, 1), (0, 2), (1, 2)], moves: (0, 0) },
            Piece { cells: 93, kind: PieceKind::Grows, form: Vec::new(), moves: (0, 0) },
            flying((2, 0)),
            Piece { cells: 1, kind: PieceKind::Oscillator { period: 4 }, form: vec![(0, 0)], moves: (0, 0) },
        ];
        let [ships, oscillators, still] = listed(&apart);
        // One kind of ship, two of them flying right and one left; what goes round, by its
        // size; and what keeps still, which has no period.
        let said = |kind: &Listed| (kind.title.clone(), kind.note, kind.period, kind.cells, kind.count);
        assert_eq!(ships.iter().map(said).collect::<Vec<_>>(), [("c/6".to_string(), "orthogonal", Some(12), 4, 3)]);
        assert_eq!(ships[0].ways, [0, 0, 2, 0, 0, 0, 1, 0]);
        assert_eq!(oscillators.iter().map(said).collect::<Vec<_>>(), [("1×1".to_string(), "", Some(4), 1, 2)]);
        assert_eq!(still.iter().map(said).collect::<Vec<_>>(), [("2×2".to_string(), "", None, 4, 1)]);
        assert_eq!((oscillators[0].ways, still[0].ways), ([0; 8], [0; 8]));
        assert_eq!(
            others(&apart, 2),
            [(1, "one that grows", "that grow"), (2, "one more not followed", "more not followed")]
        );
        // And summed up, a tile to each sort, with its sign.
        let said = |sort: &Summary| (sort.count, sort.label, sort.icon, sort.kinds.clone());
        assert_eq!(
            sorts(&apart, 2).iter().map(said).collect::<Vec<_>>(),
            [
                (3, "SPACESHIPS", icons::SHIP, "c/6".to_string()),
                (2, "OSCILLATORS", icons::OSCILLATES, "period 4".to_string()),
                (1, "STILL LIFE", icons::STILL, String::new()),
                (1, "THAT GROWS", icons::GROWS, String::new()),
                (2, "MORE, NOT FOLLOWED", icons::UNDECIDED, String::new()),
            ]
        );
        // The rows of the list, in their order: what a mark on a row keeps.
        let kinds = listed(&apart);
        let rows: Vec<(Sort, usize, (i32, i32))> =
            rows_of(&kinds).iter().map(|(sort, kind)| (*sort, kind.cells, kind.moves)).collect();
        assert_eq!(rows, [(Sort::Spaceship, 4, (2, 0)), (Sort::Oscillator, 1, (0, 0)), (Sort::StillLife, 4, (0, 0))]);
    }

    #[test]
    fn the_commonest_kinds_are_named() {
        let kinds = |counts: &[(&str, usize)]| counts.iter().map(|&(name, count)| (name.to_string(), count)).collect();
        let few: Vec<(String, usize)> = kinds(&[("16", 22), ("4", 215)]);
        assert_eq!(commonest(&few, "one more", "more"), "4\u{a0}×215, 16\u{a0}×22");
        let many: Vec<(String, usize)> = kinds(&[("8", 8), ("4", 215), ("16", 22), ("28", 4), ("36", 2)]);
        assert_eq!(commonest(&many, "one more", "more"), "4\u{a0}×215, 16\u{a0}×22, 8\u{a0}×8 and\u{a0}2\u{a0}more");
        let ships: Vec<(String, usize)> = kinds(&[("c/3 ↖↗↙↘", 189)]);
        assert_eq!(commonest(&ships, "one more kind", "more kinds"), "c/3 ↖↗↙↘");
        let four: Vec<(String, usize)> = kinds(&[("a", 1), ("b", 1), ("c", 1), ("d", 1)]);
        assert_eq!(commonest(&four, "one more kind", "more kinds"), "a, b, c and\u{a0}one\u{a0}more\u{a0}kind");
    }
}
