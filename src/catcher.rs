//! The spaceship catcher: a panel listing the patterns caught at the edge of the grid.
//!
//! While the universe is catching ([`Universe::catching`]), the small patterns that reach its
//! edge are taken out of the world and handed over as [`Departure`]s. Here they are identified
//! and counted by kind ([`Census`]), a little every frame, and listed. Every rule has a haul of
//! its own.

use std::collections::{HashMap, VecDeque};

use bevy::{
    clipboard::Clipboard,
    feathers::{
        constants::fonts,
        controls::{FeathersButton, FeathersScrollbar},
        cursor::EntityCursor,
        palette,
        theme::{ThemeTextColor, ThemedText},
        tokens,
    },
    picking::hover::Hovered,
    platform::time::Instant,
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
    ui_widgets::{Activate, ControlOrientation, ScrollArea},
    window::SystemCursorIcon,
};

use cas_core::{
    census::{Census, Kind},
    pattern::{Cell, Heading, to_rle},
    rules::BlockRule,
    universe::{Departure, Universe},
};

use crate::{
    actions::Toggle,
    sim::{SimSystems, rule_changed},
    ui::{Aspect, caption, group_digits, panel_title, side_panel, toggle},
    view::{ALIVE, DEAD},
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

/// Column widths of the list, shared by its header and its rows; the speed takes the rest.
const PICTURE: (f32, f32) = (64.0, 44.0);
const PERIOD_COLUMN: f32 = 48.0;
const CELLS_COLUMN: f32 = 38.0;
const CAUGHT_COLUMN: f32 = 58.0;

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

    /// Spaceships caught under `rule`, and how many kinds they are.
    pub fn totals(&self, rule: &BlockRule) -> (u64, usize) {
        self.hauls
            .get(rule)
            .map_or((0, 0), |haul| (haul.census.ships(), haul.census.kinds().len()))
    }
}

/// Everything caught under one rule.
struct Haul {
    /// Tells this haul from every other, also from an earlier one of the same rule.
    number: u64,
    census: Census,
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

/// The list's scrollbar, shown only while there is something to scroll.
#[derive(Component, Default, Clone)]
struct KindScrollbar;

/// The line under the list.
#[derive(Component, Default, Clone)]
struct Note;

pub struct CatcherPlugin;

impl Plugin for CatcherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Catcher>().add_systems(
            Update,
            (drop_the_queue.run_if(rule_changed), identify, show_panel, sync_list, light_rows)
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
    let haul = hauls.entry(universe.rule().clone()).or_insert_with(|| {
        *begun += 1;
        Haul {
            number: *begun,
            census: Census::new(universe.rule()),
        }
    });
    while let Some(departure) = waiting.pop_front() {
        haul.census.record(departure);
        if started.elapsed() >= BUDGET {
            break;
        }
    }
}

pub fn catcher_panel() -> impl Scene {
    bsn! {
        #Catcher
        side_panel(CATCHER_WIDTH, bsn_list![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                }
                Children [
                    panel_title(Aspect::Pattern, "Spaceships"),
                    (
                        #CatcherClose
                        @FeathersButton {
                            @caption: bsn! { Text("Close") ThemedText }
                        }
                        on(|_: On<Activate>, mut catcher: ResMut<Catcher>| catcher.toggle())
                    ),
                ]
            ),
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
                    column_gap: px(10),
                    padding: UiRect { left: px(8), right: px(18) },
                }
                Children [
                    (Node { width: px(PICTURE.0) } Children [ heading("PATTERN") ]),
                    (Node { flex_grow: 1.0, flex_basis: px(0) } Children [ heading("SPEED") ]),
                    (Node { width: px(PERIOD_COLUMN), justify_content: JustifyContent::End } Children [ heading("PERIOD") ]),
                    (Node { width: px(CELLS_COLUMN), justify_content: JustifyContent::End } Children [ heading("CELLS") ]),
                    (Node { width: px(CAUGHT_COLUMN), justify_content: JustifyContent::End } Children [ heading("CAUGHT") ]),
                ]
            ),
            (
                // The frame holds the scrollbar; the list inside it scrolls.
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect { right: px(10) },
                }
                Children [
                    (
                        #KindList
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4),
                            overflow: Overflow::scroll_y(),
                        }
                        ScrollArea
                        KindList
                    ),
                    (
                        @FeathersScrollbar {
                            @target: #KindList,
                            @orientation: {ControlOrientation::Vertical}
                        }
                        KindScrollbar
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            right: px(0),
                            top: px(0),
                            bottom: px(0),
                            width: px(6),
                        }
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
                        #CatcherForget
                        @FeathersButton {
                            @caption: bsn! { Text("Clear list") ThemedText }
                        }
                        Node { flex_shrink: 0.0 }
                        on(|_: On<Activate>, universe: Res<Universe>, mut catcher: ResMut<Catcher>| {
                            catcher.hauls.remove(universe.rule());
                            catcher.note = None;
                        })
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
            (
                Text("0")
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::MONO),
                    font_size: FontSize::Px(18.0),
                    weight: FontWeight::NORMAL,
                }
                TextColor(palette::WHITE)
                template_value(figure)
            ),
            caption(label),
        ]
    }
}

/// A column title of the list.
fn heading(title: &'static str) -> impl Scene {
    bsn! {
        Text(title)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::BOLD),
            font_size: FontSize::Px(10.0),
            weight: FontWeight::BOLD,
        }
        ThemeTextColor(tokens::TEXT_DIM)
    }
}

fn mono(text: String, size: f32, color: Color) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::MONO),
            font_size: FontSize::Px(size),
            weight: FontWeight::NORMAL,
        }
        TextColor(color)
        template_value(Pickable::IGNORE)
    }
}

/// One kind of spaceship: its picture, how it moves, and how often it was caught, out of
/// `ships` in all.
fn kind_row(index: usize, kind: &Kind, ships: u64) -> impl Scene {
    let motion = &kind.motion;
    let name = Name::new(format!("Kind{index}"));
    let row = KindRow(index);
    let (caught, share) = (Figure::Caught(index), Share(index));
    let bar = Aspect::Pattern.color();
    let (travelled, period) = motion.speed();
    // Which way it flies, by the signs of its displacement.
    const ARROWS: [[&str; 3]; 3] = [["↖", "↑", "↗"], ["←", "", "→"], ["↙", "↓", "↘"]];
    let (dx, dy) = motion.displacement;
    let arrow = ARROWS[(dy.signum() + 1) as usize][(dx.signum() + 1) as usize];
    let speed = match (travelled, period) {
        (1, 1) => format!("c {arrow}"),
        (1, _) => format!("c/{period} {arrow}"),
        _ => format!("{travelled}c/{period} {arrow}"),
    };
    let heading = match motion.heading() {
        Heading::Orthogonal => "orthogonal",
        Heading::Diagonal => "diagonal",
        Heading::Oblique => "oblique",
        Heading::Still => "still",
    };
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::axes(px(8), px(6)),
            border_radius: px(5),
            flex_shrink: 0.0,
        }
        BackgroundColor(palette::GRAY_2)
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        template_value(name)
        template_value(row)
        on(copy_kind)
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                }
                template_value(Pickable::IGNORE)
                Children [
                    picture(&motion.canonical),
                    (
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                        }
                        template_value(Pickable::IGNORE)
                        Children [
                            mono(speed, 14.0, palette::WHITE),
                            caption(heading),
                        ]
                    ),
                    number(motion.period.to_string(), PERIOD_COLUMN, palette::LIGHT_GRAY_1),
                    number(motion.canonical.len().to_string(), CELLS_COLUMN, palette::LIGHT_GRAY_1),
                    (
                        number(group_digits(kind.count as i64), CAUGHT_COLUMN, palette::WHITE)
                        template_value(caught)
                    ),
                ]
            ),
            (
                Node {
                    height: px(3),
                    border_radius: BorderRadius::MAX,
                }
                BackgroundColor(palette::GRAY_0)
                template_value(Pickable::IGNORE)
                Children [(
                    Node {
                        width: percent(100.0 * share_of(kind, ships)),
                        height: percent(100),
                        border_radius: BorderRadius::MAX,
                    }
                    BackgroundColor(bar)
                    template_value(Pickable::IGNORE)
                    template_value(share)
                )]
            ),
        ]
    }
}

/// A figure set to the right of its column.
fn number(text: String, column: f32, color: Color) -> impl Scene {
    bsn! {
        mono(text, 12.0, color)
        TextLayout { justify: Justify::Right }
        Node { width: px(column) }
    }
}

/// The pattern drawn small: one square per cell, as large as fits the box.
fn picture(cells: &[Cell]) -> impl Scene {
    let left = cells.iter().map(|cell| cell.0).min().unwrap_or(0);
    let top = cells.iter().map(|cell| cell.1).min().unwrap_or(0);
    let width = cells.iter().map(|cell| cell.0 - left + 1).max().unwrap_or(1) as f32;
    let height = cells.iter().map(|cell| cell.1 - top + 1).max().unwrap_or(1) as f32;
    let side = ((PICTURE.0 - 12.0) / width)
        .min((PICTURE.1 - 12.0) / height)
        .floor()
        .clamp(1.0, 7.0);
    // A hairline between neighbours, once the squares are big enough to spare it.
    let ink = if side >= 4.0 { side - 1.0 } else { side };
    let squares: Vec<_> = cells
        .iter()
        .map(|&(x, y)| {
            let (x, y) = ((x - left) as f32 * side, (y - top) as f32 * side);
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x),
                    top: px(y),
                    width: px(ink),
                    height: px(ink),
                }
                BackgroundColor(ALIVE)
            }
        })
        .collect();
    bsn! {
        Node {
            width: px(PICTURE.0),
            height: px(PICTURE.1),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: px(4),
        }
        BackgroundColor(DEAD)
        template_value(Pickable::IGNORE)
        Children [(
            Node {
                width: px(width * side),
                height: px(height * side),
            }
            Children [ { squares } ]
        )]
    }
}

/// A click on a row puts the pattern on the clipboard as text.
fn copy_kind(
    click: On<Pointer<Click>>,
    rows: Query<&KindRow>,
    universe: Res<Universe>,
    mut clipboard: ResMut<Clipboard>,
    mut catcher: ResMut<Catcher>,
) {
    let Ok(&KindRow(index)) = rows.get(click.entity) else {
        return;
    };
    let Some(kind) = catcher
        .hauls
        .get(universe.rule())
        .and_then(|haul| haul.census.kinds().get(index))
    else {
        return;
    };
    let rle = to_rle(&kind.motion.canonical);
    catcher.note = Some(match clipboard.set_text(rle.as_str()) {
        Ok(()) => format!("Copied {rle}"),
        Err(error) => format!("The clipboard is not available ({error:?})."),
    });
}

/// A row lights up under the pointer: a click on it does something.
fn light_rows(mut rows: Query<(&Hovered, &mut BackgroundColor), (With<KindRow>, Changed<Hovered>)>) {
    for (hovered, mut background) in &mut rows {
        background.0 = if hovered.0 { palette::GRAY_3 } else { palette::GRAY_2 };
    }
}

/// Shows or hides the panel, and its scrollbar while there is something to scroll.
fn show_panel(
    catcher: Res<Catcher>,
    mut panel: Single<&mut Node, With<CatcherPanel>>,
    list: Single<&ComputedNode, With<KindList>>,
    mut scrollbar: Single<&mut Node, (With<KindScrollbar>, Without<CatcherPanel>)>,
) {
    let display = |shown: bool| if shown { Display::Flex } else { Display::None };
    if panel.display != display(catcher.open) {
        panel.display = display(catcher.open);
    }
    let scrolls = list.content_size().y > list.size().y + 0.5;
    if scrollbar.display != display(scrolls) {
        scrollbar.display = display(scrolls);
    }
}

/// What the list last showed, to tell when it is out of date.
#[derive(PartialEq)]
struct Shown {
    /// The number of the haul the rows are of.
    haul: Option<u64>,
    caught: u64,
    kinds: usize,
    catching: bool,
    note: Option<String>,
}

/// Keeps the figures and the list up to date, at most every so often, since catches can come
/// in by the hundred. Rows stay for as long as their haul is shown: its kinds only get more.
fn sync_list(
    catcher: Res<Catcher>,
    universe: Res<Universe>,
    time: Res<Time<Real>>,
    list: Single<(Entity, Option<&Children>), With<KindList>>,
    rows: Query<(Entity, &KindRow)>,
    mut figures: Query<(&Figure, &mut Text), Without<Note>>,
    mut shares: Query<(&Share, &mut Node)>,
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
        catching: universe.catching,
        note: catcher.note.clone(),
    };
    let same_haul = shown.as_ref().is_some_and(|shown| shown.haul == now.haul);
    if shown.as_ref() == Some(&now) || (*wait > 0.0 && same_haul) {
        return;
    }
    let hint = match (now.kinds, now.catching) {
        (0, false) => "Catching is off.",
        (0, true) => "No spaceships yet: let a blob run.",
        _ => "Click a pattern to copy it as text.",
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
