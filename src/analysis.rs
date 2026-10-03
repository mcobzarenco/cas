//! Analysing a pattern: a panel that runs a pattern of the grid on its own and says what it
//! does.
//!
//! A pattern is chosen by dragging a band around it on the grid (after **Analyse**), or sent
//! over from the spaceship list. It is followed on the unbounded plane until it repeats or
//! gets out of hand ([`Analyser::study`]), and shown living in a small world of its own, a
//! torus just big enough for it.

use bevy::{
    clipboard::Clipboard,
    feathers::{
        constants::fonts,
        controls::FeathersButton,
        theme::{ThemeTextColor, ThemedText},
        tokens,
    },
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
    ui_widgets::Activate,
};

use cas_core::{
    pattern::{Analyser, Cell, Fate, Heading, Study, to_rle},
    rules::BlockRule,
    universe::Universe,
};

use crate::{
    sim::{Settings, SimSystems},
    ui::{Aspect, caption, panel_title, side_panel},
    view::{EDGE, Framing, GridMaterial, GridParams, Stamp, cell_image, upload},
};

pub const ANALYSIS_WIDTH: f32 = 396.0;

/// The size of the small world's view, in logical pixels; the world has the same proportions.
const VIEW: (f32, f32) = (360.0, 270.0);
/// The small world runs at this many generations a second at least, and faster for a pattern
/// with a long period, so that a period takes about this long; but no faster than this.
const PACE: (f32, f32) = (10.0, 120.0);
const PERIOD_SECONDS: f32 = 1.5;
/// The small world is this many times as wide as the pattern gets, within these bounds, and
/// in any case wide enough for the pattern as it set out.
const ROOM: i32 = 3;
const WORLD_SIDES: (usize, usize) = (32, 256);
/// How far a pattern is followed: so many generations times cells, a large pattern for fewer
/// generations so that the study takes no time, within these bounds; and until it has so
/// many cells or is so wide.
const WORK: u64 = 2_000_000;
const GENERATIONS: (u32, u32) = (256, 8192);
const MOST_CELLS: usize = 1024;
const WIDEST: i32 = 512;

#[derive(Resource, Default)]
pub struct Analysis {
    open: bool,
    /// A pattern is being chosen on the grid: the next left-drag draws a band around it.
    pub selecting: bool,
    /// The band, from the cell pressed to the cell under the pointer.
    pub band: Option<(IVec2, IVec2)>,
    subject: Option<Subject>,
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
    rle: String,
}

impl Analysis {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.stop_choosing();
        }
    }

    /// Choosing a pattern on the grid begins, or is called off; it opens the panel, which
    /// says what to do.
    pub fn choose(&mut self) {
        self.selecting = !self.selecting;
        self.band = None;
        self.note = None;
        if self.selecting {
            self.open = true;
        }
    }

    pub fn stop_choosing(&mut self) {
        if self.selecting || self.band.is_some() {
            self.selecting = false;
            self.band = None;
        }
    }

    /// Studies a pattern, given relative to a corner of the blocks the next step rewrites, as
    /// a pattern caught at the edge is, with the vacuum `phase` generations into its cycle.
    pub fn study(&mut self, cells: Vec<Cell>, phase: usize, universe: &Universe) {
        self.selecting = false;
        self.band = None;
        self.open = true;
        let rule = universe.rule();
        let mut analyser = Analyser::new(rule);
        analyser.max_cells = MOST_CELLS.max(4 * cells.len());
        analyser.max_extent = WIDEST;
        let generations = WORK / cells.len().max(1) as u64;
        analyser.max_generations = generations.clamp(GENERATIONS.0 as u64, GENERATIONS.1 as u64) as u32;
        let Some(study) = analyser.study(&cells, phase) else {
            self.note = Some("No live cells in there.".to_string());
            return;
        };
        let pace = match &study.motion {
            Some(motion) => (motion.period as f32 / PERIOD_SECONDS).clamp(PACE.0, PACE.1),
            None => PACE.0,
        };
        self.studied += 1;
        self.subject = Some(Subject {
            number: self.studied,
            forms: analyser.forms(&study.start),
            world: small_world(&study, rule),
            rle: to_rle(&study.start),
            rule: rule.clone(),
            clock: 0.0,
            pace,
            study,
        });
        self.note = None;
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

/// One thing the study says.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Finding {
    #[default]
    What,
    Period,
    Speed,
    Cells,
    Size,
    Parts,
    Text,
}

/// The line under the buttons.
#[derive(Component, Default, Clone)]
struct Note;

/// The caption next to the Analyse button of the pattern card: what to do next.
#[derive(Component, Default, Clone)]
pub struct SelectHint;

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
                (run_small_world, draw_small_world, sync_panel)
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
                    panel_title(Aspect::Pattern, "Analysis"),
                    (
                        #AnalysisClose
                        @FeathersButton {
                            @caption: bsn! { Text("Close") ThemedText }
                        }
                        on(|_: On<Activate>, mut analysis: ResMut<Analysis>| analysis.toggle())
                    ),
                ]
            ),
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
            (#SmallCaption caption("") SmallCaption),
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
                    line("SIZE", Finding::Size),
                    line("PARTS", Finding::Parts),
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
        ])
        AnalysisPanel
    }
}

/// One finding: its name and, next to it, what was found.
fn line(label: &'static str, finding: Finding) -> impl Scene {
    let name = Name::new(format!("Study{finding:?}"));
    // The mono font has the diagonal arrows, and the text is in it anyway.
    let font = if matches!(finding, Finding::Speed | Finding::Text) { fonts::MONO } else { fonts::REGULAR };
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Baseline,
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
                    width: px(56),
                    flex_shrink: 0.0,
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

/// The pattern goes back on the grid: picked up, to be put down wherever.
fn place_subject(_: On<Activate>, mut analysis: ResMut<Analysis>, mut stamp: ResMut<Stamp>) {
    if let Some(subject) = &analysis.subject {
        stamp.pick_up(subject.forms.clone(), &subject.rule, None);
        analysis.note = None;
    }
}

fn copy_subject(_: On<Activate>, mut analysis: ResMut<Analysis>, mut clipboard: ResMut<Clipboard>) {
    let Some(rle) = analysis.subject.as_ref().map(|subject| subject.rle.clone()) else {
        return;
    };
    analysis.note = Some(match clipboard.set_text(rle.as_str()) {
        Ok(()) => format!("Copied {rle}"),
        Err(error) => format!("The clipboard is not available ({error:?})."),
    });
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
    if !analysis.open {
        return;
    }
    if let Some(subject) = analysis.bypass_change_detection().subject.as_mut() {
        subject.clock += time.delta_secs() * subject.pace;
        let steps = subject.clock.floor();
        if steps >= 1.0 {
            subject.world.step_by(steps as i64);
            subject.clock -= steps;
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
    let world = analysis.subject.as_ref().map_or(&assets.empty, |subject| &subject.world);
    let size = node.size * node.inverse_scale_factor;
    if size.min_element() < 1.0 {
        return;
    }
    if let Some(mut image) = images.get_mut(&assets.image) {
        upload(world, &mut image);
    }
    let framing = Framing::fitted(world, size, 1.0 / node.inverse_scale_factor);
    let params = GridParams::new(world, framing, &settings, EDGE, None, None);
    GridMaterial::set(&mut materials, &assets.material, params);
}

/// What the panel last showed, to tell when it is out of date.
#[derive(PartialEq)]
struct Shown {
    subject: Option<u64>,
    selecting: bool,
    /// The subject is picked up, to be put down on the grid.
    holding: bool,
    note: Option<String>,
}

/// Shows or hides the panel, and keeps its texts and the hint of the pattern card in step.
fn sync_panel(
    analysis: Res<Analysis>,
    stamp: Res<Stamp>,
    mut panel: Single<&mut Node, With<AnalysisPanel>>,
    mut findings: Query<(&Finding, &mut Text)>,
    mut captions: Query<&mut Text, (With<SmallCaption>, Without<Finding>, Without<Note>, Without<SelectHint>)>,
    mut note: Single<&mut Text, (With<Note>, Without<Finding>, Without<SelectHint>)>,
    mut hint: Single<&mut Text, (With<SelectHint>, Without<Finding>, Without<Note>)>,
    mut shown: Local<Option<Shown>>,
) {
    let display = if analysis.open { Display::Flex } else { Display::None };
    if panel.display != display {
        panel.display = display;
    }
    let now = Shown {
        subject: analysis.subject.as_ref().map(|subject| subject.number),
        selecting: analysis.selecting,
        // A stamp from the list is the list's business.
        holding: stamp.is_held() && stamp.kind.is_none(),
        note: analysis.note.clone(),
    };
    if shown.as_ref() == Some(&now) {
        return;
    }
    let now = shown.insert(now);

    let subject = analysis.subject.as_ref();
    for (finding, mut text) in &mut findings {
        let content = subject.map_or("—".to_string(), |subject| found(*finding, subject));
        text.set_if_neq(Text(content));
    }
    for mut text in &mut captions {
        let content = subject.map_or(String::new(), |subject| {
            let (width, height) = (subject.world.width, subject.world.height);
            format!("{width}×{height} cells of its own, {} generations a second", subject.pace.round())
        });
        text.set_if_neq(Text(content));
    }
    let what_next = match (analysis.selecting, subject.is_some(), now.holding) {
        (true, ..) => "Drag over the pattern on the grid. Escape calls it off.",
        (false, false, _) => "Nothing yet.",
        (false, true, false) => "Place picks the pattern up, to be put down on the grid where you click.",
        (false, true, true) => "Click the grid to put the pattern down, as often as you like. Escape or a right click lets go of it.",
    };
    note.set_if_neq(Text(analysis.note.clone().unwrap_or(what_next.to_string())));
    let hint_text = if analysis.selecting { "drag over it · Escape cancels" } else { "on the grid, or from the list" };
    hint.set_if_neq(Text(hint_text.to_string()));
}

/// What the study says, in words.
fn found(finding: Finding, subject: &Subject) -> String {
    let study = &subject.study;
    let motion = study.motion.as_ref();
    let travels = motion.is_some_and(|motion| motion.heading() != Heading::Still);
    let generations = |n: u32| format!("{n} generation{}", if n == 1 { "" } else { "s" });
    match finding {
        Finding::What => match (study.still, travels, study.fate) {
            (true, ..) => "Still life".to_string(),
            (_, true, _) => "Spaceship".to_string(),
            (_, _, Fate::Returns { .. }) => "Oscillator".to_string(),
            (_, _, Fate::Grows) => {
                format!("Grows: {} cells after {}", study.cells.1, generations(study.generations))
            }
            (_, _, Fate::Scatters) => format!(
                "Flies apart: {} cells across after {}",
                study.extent.0.max(study.extent.1),
                generations(study.generations)
            ),
            (_, _, Fate::Undecided) => format!("Undecided after {}", generations(study.generations)),
        },
        Finding::Period => match (study.still, study.fate) {
            (true, _) => generations(1),
            (false, Fate::Returns { period, displacement: (dx, dy) }) if travels => {
                format!("{}, moving ({dx}, {dy})", generations(period))
            }
            (false, Fate::Returns { period, .. }) => generations(period),
            _ => "—".to_string(),
        },
        Finding::Speed => match (motion, study.fate) {
            // The motion is that of the canonical form; the way it goes is as it lies.
            (Some(motion), Fate::Returns { displacement: (dx, dy), .. }) if travels => {
                let speed = match motion.speed() {
                    (1, 1) => "c".to_string(),
                    (1, period) => format!("c/{period}"),
                    (cells, period) => format!("{cells}c/{period}"),
                };
                let heading = match motion.heading() {
                    Heading::Orthogonal => "orthogonal",
                    Heading::Diagonal => "diagonal",
                    _ => "oblique",
                };
                format!("{speed} {heading} {}", arrow(dx, dy))
            }
            _ => "—".to_string(),
        },
        Finding::Cells => match study.cells {
            (fewest, most) if fewest == most => fewest.to_string(),
            (fewest, most) => format!("{fewest} to {most}"),
        },
        Finding::Size => {
            let (width, height) = bounding_box(&study.start);
            if (width, height) == study.extent {
                format!("{width}×{height}")
            } else {
                format!("{width}×{height}, up to {}×{}", study.extent.0, study.extent.1)
            }
        }
        Finding::Parts => match study.parts {
            0 => "—".to_string(),
            1 => "one piece".to_string(),
            parts => format!("{parts} that never meet"),
        },
        Finding::Text => subject.rle.clone(),
    }
}

/// Which way a displacement points.
fn arrow(dx: i32, dy: i32) -> &'static str {
    const ARROWS: [[&str; 3]; 3] = [["↖", "↑", "↗"], ["←", "·", "→"], ["↙", "↓", "↘"]];
    ARROWS[(dy.signum() + 1) as usize][(dx.signum() + 1) as usize]
}
