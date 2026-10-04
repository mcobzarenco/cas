//! The spaceship catcher: a panel listing the patterns caught at the edge of the grid.
//!
//! While the universe is catching ([`Universe::catching`]), the small patterns that reach its
//! edge are taken out of the world and handed over as [`Departure`]s. Here they are identified
//! and counted by kind ([`Census`]), a little every frame, and listed. Every rule has a haul of
//! its own. A click on a kind picks it up, to be put back on the grid ([`Stamp`]).
//!
//! A catch that does not repeat in the time a frame can spare is followed for much longer on
//! another thread, one catch at a time: there are spaceships that take thousands of
//! generations, or millions, to be back in their shape.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use bevy::{
    clipboard::Clipboard,
    feathers::{controls::FeathersButton, palette},
    platform::time::Instant,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    ui_widgets::Activate,
};

use cas_core::{
    census::{self, Census, Found, Kind},
    collection::Sort,
    pattern::{Analyser, Heading, Watch, to_rle},
    rules::BlockRule,
    universe::{Departure, Universe},
};
use cas_ui::{
    Aspect, CELLS_COLUMN, COLUMN_GAP, Flown, PERIOD_COLUMN, PICTURE, Scrolls, button, caption, dial, fitting, glow,
    group_digits, heading, icon_button_marked, icons, list_row, mono, number, panel_header, panel_title, picture,
    scrolling, share_bar_marked, side_panel,
};

use crate::{
    actions::Toggle,
    analysis::Analysis,
    kept::Collected,
    sim::{SimSystems, rule_changed},
    ui::toggle,
    view::Stamp,
};

pub const CATCHER_WIDTH: f32 = 396.0;

/// How long a frame may spend identifying what was caught; the rest waits for the next one.
const BUDGET: std::time::Duration = std::time::Duration::from_millis(3);
/// The list is brought up to date at most this often, however fast the catches come in.
const REFRESH: f32 = 0.25;
/// Only so many kinds are listed, the most frequent ones.
const LISTED: usize = 200;

/// When more than this many departures wait to be identified, the oldest are let go.
const QUEUE: usize = 4096;
/// A catch that has not repeated in the time a census gives it is followed for so many
/// generations on another thread: a few seconds of work for a handful of cells, and enough
/// for a spaceship of several million generations.
const PATIENCE: u32 = 1 << 23;
/// So many catches at most wait for that longer look. Of more, the oldest are counted as no
/// spaceships: whatever leaves them faster than they can be followed is not ships.
const FOLLOWED: usize = 64;

/// The columns of this list besides those every list of patterns has. The speed takes the
/// rest: so much, next to its dial.
const SPEED_ROOM: f32 = 62.0;
pub(crate) const CAUGHT_COLUMN: f32 = 48.0;
const ANALYSE_COLUMN: f32 = 24.0;
/// The height of the two small buttons of a row, which lie one above the other.
const SMALL_BUTTON: f32 = 20.0;

#[derive(Resource, Default)]
pub struct Catcher {
    open: bool,
    /// Left the grid, not yet identified.
    waiting: VecDeque<Departure>,
    hauls: HashMap<BlockRule, Haul>,
    /// How many hauls were begun so far, which numbers them.
    begun: u64,
    /// What the last click on the list did.
    note: Option<String>,
}

impl Catcher {
    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Spaceships caught under `rule`, and how many kinds they are.
    pub fn totals(&self, rule: &BlockRule) -> (u64, usize) {
        self.hauls.get(rule).map_or((0, 0), |haul| (haul.census.ships(), haul.census.kinds().len()))
    }
}

/// Everything caught under one rule.
struct Haul {
    /// Tells this haul from every other, also from an earlier one of the same rule.
    number: u64,
    census: Census,
    /// Catches that did not repeat in time, waiting for a longer look; and the one that is
    /// having it.
    slow: VecDeque<Departure>,
    following: Option<Following>,
}

/// A catch being followed for longer, on another thread.
struct Following {
    departure: Departure,
    watch: Arc<Watch>,
    task: Task<Found>,
}

/// Nobody waits for it any more: the list was cleared, or the program ends.
impl Drop for Following {
    fn drop(&mut self) {
        self.watch.stop();
    }
}

impl Haul {
    /// A haul under a rule, numbered after the ones before it.
    fn begin(begun: &mut u64, rule: &BlockRule) -> Self {
        *begun += 1;
        Self { number: *begun, census: Census::new(rule), slow: VecDeque::new(), following: None }
    }

    /// How many catches are yet to be told for what they are.
    fn unsettled(&self) -> usize {
        self.slow.len() + self.following.is_some() as usize
    }
}

/// A kind's part of `ships` caught in all, from 0 to 1.
fn share_of(kind: &Kind, ships: u64) -> f32 {
    kind.count as f32 / ships.max(1) as f32
}

#[derive(Component, Default, Clone)]
struct CatcherPanel;

/// The scrolling node that holds one row per kind.
#[derive(Component, Default, Clone)]
struct KindList;

/// A row of the list; the value is the kind's place in its haul.
#[derive(Component, Default, Clone, Copy)]
struct KindRow(usize);

/// The figures of the panel: the three above the list, and how often each listed kind was
/// caught.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
enum Figure {
    #[default]
    Ships,
    Kinds,
    Others,
    Caught(usize),
}

/// The bar of a row: its kind's share of all the spaceships caught.
#[derive(Component, Default, Clone, Copy)]
struct Share(usize);

/// The small button of a row that sends its kind to the analysis panel.
#[derive(Component, Default, Clone, Copy)]
struct AnalyseKind(usize);

/// The small button of a row that keeps its kind, or lets go of it, and the mark on it, which
/// is lit while the kind is kept.
#[derive(Component, Default, Clone, Copy)]
struct KeepKind(usize);

#[derive(Component, Default, Clone, Copy)]
struct KeepSign(usize);

/// The line under the list.
#[derive(Component, Default, Clone)]
struct Note;

pub struct CatcherPlugin;

impl Plugin for CatcherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Catcher>().add_systems(
            Update,
            (
                drop_the_queue.run_if(rule_changed),
                identify,
                follow_the_slow,
                show_panel,
                sync_list,
                outline_held,
                light_kept,
            )
                .chain()
                .in_set(SimSystems::Present),
        );
    }
}

/// What is still waiting left under the previous rule, and would be misjudged by this one.
fn drop_the_queue(mut catcher: ResMut<Catcher>) {
    if !catcher.waiting.is_empty() {
        catcher.waiting.clear();
    }
}

/// Queues what was caught at the edge, and identifies as much of the queue as fits.
fn identify(mut universe: ResMut<Universe>, mut catcher: ResMut<Catcher>) {
    if universe.has_departures() {
        // Handing them over changes nothing that anyone watching the universe cares about.
        let departures = universe.bypass_change_detection().take_departures();
        catcher.waiting.extend(departures);
        let excess = catcher.waiting.len().saturating_sub(QUEUE);
        catcher.waiting.drain(..excess);
    }
    if catcher.waiting.is_empty() {
        return;
    }
    let started = Instant::now();
    let Catcher { waiting, hauls, begun, .. } = &mut *catcher;
    let haul = hauls.entry(universe.rule().clone()).or_insert_with(|| Haul::begin(begun, universe.rule()));
    while let Some(departure) = waiting.pop_front() {
        if let Some(slow) = haul.census.record_or_defer(departure) {
            haul.slow.push_back(slow);
            if haul.slow.len() > FOLLOWED
                && let Some(oldest) = haul.slow.pop_front()
            {
                haul.census.count(&oldest, Vec::new());
            }
        }
        if started.elapsed() >= BUDGET {
            break;
        }
    }
}

/// Gives the catches that did not repeat at once a longer look, one at a time and on another
/// thread, and counts each for what it turned out to be.
fn follow_the_slow(universe: Res<Universe>, mut catcher: ResMut<Catcher>) {
    let Some(haul) = catcher.bypass_change_detection().hauls.get_mut(universe.rule()) else {
        return;
    };
    if let Some(following) = &mut haul.following {
        let Some(found) = check_ready(&mut following.task) else {
            return;
        };
        haul.census.count(&following.departure, found.ships);
        haul.following = None;
    }
    let Some(departure) = haul.slow.pop_front() else {
        return;
    };
    let watch = Arc::new(Watch::default());
    let mut patient = Analyser::new(universe.rule());
    patient.max_generations = PATIENCE;
    patient.watch = Some(watch.clone());
    let (cells, phase) = (departure.cells.clone(), departure.phase);
    let task = AsyncComputeTaskPool::get().spawn(async move { census::identify(&patient, &cells, phase) });
    haul.following = Some(Following { departure, watch, task });
}

pub fn catcher_panel() -> impl Scene {
    bsn! {
        #Catcher
        side_panel(CATCHER_WIDTH, bsn_list![
            panel_header(panel_title(Aspect::Pattern, "Caught"), bsn! {
                #CatcherClose
                on(|_: On<Activate>, mut catcher: ResMut<Catcher>| catcher.toggle())
            }),
            caption("Small patterns that reach the edge of the grid are taken out of the world. Those that travel are identified and counted here."),
            toggle("Catch spaceships", "CatcherCatching", Toggle::Catching),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    figure("spaceships", Figure::Ships),
                    figure("kinds", Figure::Kinds),
                    figure("others", Figure::Others),
                ]
            ),
            (
                // Titles over the columns of the rows below: same widths, same padding.
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(COLUMN_GAP),
                    padding: UiRect { left: px(8), right: px(18) },
                }
                Children [
                    (Node { width: px(PICTURE.0) } Children [ heading("PATTERN") ]),
                    (Node { flex_grow: 1.0, flex_basis: px(0) } Children [ heading("SPEED") ]),
                    (Node { width: px(PERIOD_COLUMN), justify_content: JustifyContent::End } Children [ heading("PERIOD") ]),
                    (Node { width: px(CELLS_COLUMN), justify_content: JustifyContent::End } Children [ heading("CELLS") ]),
                    (Node { width: px(CAUGHT_COLUMN), justify_content: JustifyContent::End } Children [ heading("CAUGHT") ]),
                    (Node { width: px(ANALYSE_COLUMN) }),
                ]
            ),
            scrolling(Scrolls::Rows, bsn! { #KindList KindList }),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                }
                Children [
                    (
                        #CatcherForget
                        button("Clear list")
                        Node { flex_shrink: 0.0 }
                        on(|_: On<Activate>, universe: Res<Universe>, mut catcher: ResMut<Catcher>| {
                            catcher.hauls.remove(universe.rule());
                            catcher.note = None;
                        })
                    ),
                    (
                        // Keeps every kind of the list among the spaceships of the rule.
                        #CatcherKeepAll
                        button("Keep all")
                        Node { flex_shrink: 0.0 }
                        on(keep_all_kinds)
                    ),
                    (#CatcherNote caption("") Note Node { flex_grow: 1.0, flex_basis: px(0) }),
                ]
            ),
        ])
        CatcherPanel
    }
}

/// A number with what it counts underneath.
fn figure(label: &'static str, figure: Figure) -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(2),
            padding: UiRect::axes(px(6), px(8)),
            border_radius: px(5),
        }
        BackgroundColor(palette::GRAY_2)
        Children [
            (mono("0", 18.0, palette::WHITE) template_value(figure)),
            caption(label),
        ]
    }
}

/// One kind of spaceship: its picture, how it moves, and how often it was caught, out of
/// `ships` in all.
fn kind_row(index: usize, kind: &Kind, ships: u64) -> impl Scene {
    let motion = &kind.motion;
    let name = Name::new(format!("Kind{index}"));
    let row = KindRow(index);
    let (caught, share) = (Figure::Caught(index), Share(index));
    let (analyse, analyse_name) = (AnalyseKind(index), Name::new(format!("AnalyseKind{index}")));
    let (keep, keep_name) = (KeepKind(index), Name::new(format!("KeepKind{index}")));
    // The picture is of the form the kind is filed under, which flies right or down; the ways
    // its ships were going when they were caught are on the dial.
    let speed = match motion.speed() {
        (1, 1) => "c".to_string(),
        (1, period) => format!("c/{period}"),
        (travelled, period) => format!("{travelled}c/{period}"),
    };
    let speed_size = fitting(speed.chars().count(), SPEED_ROOM, 14.0);
    let heading = match motion.heading() {
        Heading::Orthogonal => "orthogonal",
        Heading::Diagonal => "diagonal",
        Heading::Oblique => "oblique",
        Heading::Still => "still",
    };
    bsn! {
        list_row()
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
        }
        template_value(name)
        template_value(row)
        on(pick_kind)
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(COLUMN_GAP),
                }
                template_value(Pickable::IGNORE)
                Children [
                    picture(&motion.canonical),
                    (
                        // Whatever is left; a speed too long for it is cut rather than let
                        // widen the row, and the panel with it.
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_width: px(0),
                            overflow: Overflow::clip(),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                        }
                        template_value(Pickable::IGNORE)
                        Children [
                            mono(speed, speed_size, palette::WHITE),
                            caption(heading),
                        ]
                    ),
                    dial(&kind.ways, Some(index)),
                    number(motion.period.to_string(), PERIOD_COLUMN, palette::LIGHT_GRAY_1),
                    number(motion.canonical.len().to_string(), CELLS_COLUMN, palette::LIGHT_GRAY_1),
                    (
                        number(group_digits(kind.count as i64), CAUGHT_COLUMN, palette::WHITE)
                        template_value(caught)
                    ),
                    (
                        // One above the other: a closer look in the analysis panel, and the
                        // mark that keeps the kind, lit while it is kept.
                        Node {
                            width: px(ANALYSE_COLUMN),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(3),
                            flex_shrink: 0.0,
                        }
                        Children [
                            (
                                @FeathersButton {
                                    @caption: bsn! { icons::icon(icons::LOOK, 12.0, palette::LIGHT_GRAY_1) }
                                }
                                Node {
                                    width: px(ANALYSE_COLUMN),
                                    min_width: px(ANALYSE_COLUMN),
                                    height: px(SMALL_BUTTON),
                                    min_height: px(SMALL_BUTTON),
                                    padding: px(0),
                                    justify_content: JustifyContent::Center,
                                    flex_shrink: 0.0,
                                }
                                template_value(analyse_name)
                                template_value(analyse)
                                on(analyse_kind)
                            ),
                            (
                                icon_button_marked(icons::KEEP, palette::LIGHT_GRAY_2, KeepSign(index))
                                Node { height: px(SMALL_BUTTON), min_height: px(SMALL_BUTTON) }
                                template_value(keep_name)
                                template_value(keep)
                                on(keep_kind)
                            ),
                        ]
                    ),
                ]
            ),
            share_bar_marked(share_of(kind, ships), Aspect::Pattern, share),
        ]
    }
}

/// A click on a row picks the pattern up, to be put on the grid; on the row of the pattern
/// held, it lets go of it. With shift, the click puts the pattern on the clipboard as text.
fn pick_kind(
    click: On<Pointer<Click>>,
    rows: Query<&KindRow>,
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    mut clipboard: ResMut<Clipboard>,
    mut catcher: ResMut<Catcher>,
    mut stamp: ResMut<Stamp>,
    mut analysis: ResMut<Analysis>,
) {
    let Ok(&KindRow(index)) = rows.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    let Some((haul, kind)) =
        catcher.hauls.get(universe.rule()).and_then(|haul| Some((haul.number, haul.census.kinds().get(index)?)))
    else {
        return;
    };
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        let rle = to_rle(&kind.motion.canonical);
        catcher.note = Some(match clipboard.set_text(rle.as_str()) {
            Ok(()) => format!("Copied {rle}"),
            Err(error) => format!("The clipboard is not available ({error:?})."),
        });
    } else if stamp.kind == Some((haul, index)) {
        stamp.let_go();
    } else {
        let forms = Analyser::new(universe.rule()).forms(&kind.motion.canonical);
        stamp.pick_up(forms, universe.rule(), Some((haul, index)));
        catcher.note = None;
        // A click on the grid puts the pattern down now, rather than starting a band.
        analysis.stop_choosing();
    }
}

/// The row's small button sends its kind to the analysis panel. The click goes no further:
/// the row would pick the kind up.
fn analyse_kind(
    mut click: On<Pointer<Click>>,
    buttons: Query<&AnalyseKind>,
    universe: Res<Universe>,
    catcher: Res<Catcher>,
    mut analysis: ResMut<Analysis>,
) {
    let Ok(&AnalyseKind(index)) = buttons.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if click.button != PointerButton::Primary {
        return;
    }
    if let Some(kind) = catcher.hauls.get(universe.rule()).and_then(|haul| haul.census.kinds().get(index)) {
        // A kind is filed at the start of the vacuum's cycle.
        analysis.study(kind.motion.canonical.clone(), 0, &universe);
    }
}

/// The mark of a row keeps its kind among the spaceships of the rule, or lets go of it. The
/// click goes no further: the row would pick the pattern up.
fn keep_kind(
    mut click: On<Pointer<Click>>,
    buttons: Query<&KeepKind>,
    universe: Res<Universe>,
    catcher: Res<Catcher>,
    mut collected: ResMut<Collected>,
) {
    let Ok(&KeepKind(index)) = buttons.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if click.button != PointerButton::Primary {
        return;
    }
    let rule = universe.rule();
    if let Some(kind) = catcher.hauls.get(rule).and_then(|haul| haul.census.kinds().get(index)) {
        let motion = &kind.motion;
        collected.keep_or_forget(rule, Sort::Spaceship, &motion.canonical, motion.period, motion.displacement);
    }
}

/// Keeps every kind of the list.
fn keep_all_kinds(
    _: On<Activate>,
    universe: Res<Universe>,
    mut catcher: ResMut<Catcher>,
    mut collected: ResMut<Collected>,
) {
    let rule = universe.rule();
    let kinds = catcher.hauls.get(rule).map_or(&[][..], |haul| haul.census.kinds());
    let (mut new, mut known) = (0, 0);
    for Kind { motion, .. } in kinds {
        match collected.keep(rule, Sort::Spaceship, &motion.canonical, motion.period, motion.displacement) {
            true => new += 1,
            false => known += 1,
        }
    }
    let were = if known == 1 { "was" } else { "were" };
    catcher.note = Some(match (new, known) {
        (0, 0) => "There is nothing to keep yet.".to_string(),
        (0, _) => "Every kind of the list was kept already.".to_string(),
        (1, 0) => "Kept the one kind there is.".to_string(),
        (new, 0) => format!("Kept {new} kinds."),
        (new, known) => format!("Kept {new} more, and {known} {were} kept already."),
    });
}

/// The mark of a row is lit while its kind is kept.
fn light_kept(
    catcher: Res<Catcher>,
    collected: Res<Collected>,
    universe: Res<Universe>,
    mut signs: Query<(&KeepSign, &mut TextColor)>,
    mut shown: Local<Option<(u64, usize, u64)>>,
) {
    let rule = universe.rule();
    let Some(haul) = catcher.hauls.get(rule) else {
        return;
    };
    // The signs themselves count: a row is only there a frame after its kind.
    let now = (haul.number, signs.iter().count(), collected.revision());
    if shown.replace(now) == Some(now) {
        return;
    }
    for (&KeepSign(index), mut color) in &mut signs {
        let kept = haul.census.kinds().get(index).is_some_and(|kind| collected.is_kept(rule, &kind.motion.canonical));
        color.set_if_neq(TextColor(if kept { Aspect::Pattern.color() } else { palette::LIGHT_GRAY_2 }));
    }
}

/// The row of the pattern picked up is outlined in the pattern's colour.
fn outline_held(
    stamp: Res<Stamp>,
    catcher: Res<Catcher>,
    universe: Res<Universe>,
    mut rows: Query<(&KindRow, &mut BorderColor)>,
) {
    let haul = catcher.hauls.get(universe.rule()).map(|haul| haul.number);
    for (&KindRow(index), mut border) in &mut rows {
        let held = haul.is_some_and(|haul| stamp.kind == Some((haul, index)));
        let outline = BorderColor::all(if held { Aspect::Pattern.color() } else { Color::NONE });
        if *border != outline {
            *border = outline;
        }
    }
}

/// Shows or hides the panel.
fn show_panel(catcher: Res<Catcher>, mut panel: Single<&mut Node, With<CatcherPanel>>) {
    let display = if catcher.open { Display::Flex } else { Display::None };
    if panel.display != display {
        panel.display = display;
    }
}

/// What the list last showed, to tell when it is out of date.
#[derive(PartialEq)]
struct Shown {
    /// The number of the haul the rows are of.
    haul: Option<u64>,
    caught: u64,
    kinds: usize,
    /// Catches still being followed.
    unsettled: usize,
    catching: bool,
    held: Option<(u64, usize)>,
    note: Option<String>,
}

/// Keeps the figures and the list up to date, at most every so often, since catches can come
/// in by the hundred. Rows stay for as long as their haul is shown: its kinds only get more.
fn sync_list(
    catcher: Res<Catcher>,
    universe: Res<Universe>,
    stamp: Res<Stamp>,
    time: Res<Time<Real>>,
    list: Single<(Entity, Option<&Children>), With<KindList>>,
    rows: Query<(Entity, &KindRow)>,
    mut figures: Query<(&Figure, &mut Text), Without<Note>>,
    mut shares: Query<(&Share, &mut Node)>,
    mut arrows: Query<(&Flown, &mut TextColor)>,
    mut note: Single<&mut Text, With<Note>>,
    mut shown: Local<Option<Shown>>,
    mut wait: Local<f32>,
    mut commands: Commands,
) {
    *wait -= time.delta_secs();
    if !catcher.open {
        return;
    }
    let haul = catcher.hauls.get(universe.rule());
    let census = haul.map(|haul| &haul.census);
    let now = Shown {
        haul: haul.map(|haul| haul.number),
        caught: census.map_or(0, |census| census.ships() + census.others()),
        kinds: census.map_or(0, |census| census.kinds().len()),
        unsettled: haul.map_or(0, Haul::unsettled),
        catching: universe.catching,
        held: stamp.kind,
        note: catcher.note.clone(),
    };
    let same_haul = shown.as_ref().is_some_and(|shown| shown.haul == now.haul);
    if shown.as_ref() == Some(&now) || (*wait > 0.0 && same_haul) {
        return;
    }
    let hint = match (now.held.is_some(), now.kinds, now.catching) {
        (true, ..) => {
            "Click the grid to put the pattern down, as often as you like. Escape or a right click lets go of it."
        }
        // A slow spaceship takes its time to show that it is one.
        _ if now.unsettled == 1 => "A catch has not repeated yet: it is followed further.",
        _ if now.unsettled > 1 => "Some catches have not repeated yet: they are followed further.",
        (false, 0, false) => "Catching is off.",
        (false, 0, true) => "No spaceships yet: let a blob run.",
        _ => "Click a pattern to pick it up, or shift-click it to copy it as text.",
    };
    *shown = Some(now);
    *wait = REFRESH;

    let (ships, others) = census.map_or((0, 0), |census| (census.ships(), census.others()));
    let kinds = census.map_or(&[][..], Census::kinds);
    for (figure, mut text) in &mut figures {
        let value = match *figure {
            Figure::Ships => ships,
            Figure::Kinds => kinds.len() as u64,
            Figure::Others => others,
            Figure::Caught(kind) => kinds.get(kind).map_or(0, |kind| kind.count),
        };
        text.set_if_neq(Text(group_digits(value as i64)));
    }
    for (&Share(kind), mut bar) in &mut shares {
        let width = percent(100.0 * kinds.get(kind).map_or(0.0, |kind| share_of(kind, ships)));
        if bar.width != width {
            bar.width = width;
        }
    }
    // The dials of the list: a way lights up when the first ship of its kind goes it.
    for (flown, mut color) in &mut arrows {
        if let Some(kind) = flown.kind.and_then(|kind| kinds.get(kind)) {
            color.set_if_neq(TextColor(glow(&kind.ways, flown.way)));
        }
    }
    note.set_if_neq(Text(catcher.note.clone().unwrap_or(hint.to_string())));

    // The most frequent kinds, each in the row it already has or in a new one.
    let (list, children) = *list;
    let mut rows: HashMap<usize, Entity> = rows.iter().map(|(row, kind)| (kind.0, row)).collect();
    if !same_haul {
        commands.entity(list).despawn_related::<Children>();
        rows.clear();
    }
    let mut order: Vec<usize> = (0..kinds.len()).collect();
    order.sort_by_key(|&kind| std::cmp::Reverse(kinds[kind].count));
    let listed: Vec<Entity> = order
        .into_iter()
        .take(LISTED)
        .map(|kind| {
            let row = rows.remove(&kind);
            row.unwrap_or_else(|| commands.spawn_scene(kind_row(kind, &kinds[kind], ships)).id())
        })
        .collect();
    for unlisted in rows.into_values() {
        commands.entity(unlisted).despawn();
    }
    if children.is_none_or(|children| children[..] != listed[..]) {
        commands.entity(list).replace_children(&listed);
    }
}
