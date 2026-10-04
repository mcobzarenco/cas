//! The patterns kept under a rule: the spaceships, the oscillators and the still lifes, each
//! sort in a panel of its own.
//!
//! What is kept lies in a file next to the library's ([`cas_core::collection`]), which is
//! written when something is kept or let go of, and read again whenever something else wrote
//! it ([`Synced`]). Nothing gets there by itself: a pattern is kept with a button, in the
//! analysis panel or in the list of what was caught.

use std::path::PathBuf;

use bevy::{
    feathers::{controls::FeathersButton, cursor::EntityCursor, palette, theme::ThemedText},
    picking::hover::Hovered,
    prelude::*,
    ui_widgets::Activate,
    window::SystemCursorIcon,
};

use cas_core::{
    collection::{Collection, Kept, Sort},
    pattern::{Analyser, Cell, Motion, way},
    rules::BlockRule,
    universe::Universe,
};

use crate::{
    analysis::{self, Analysis},
    kit::{
        Aspect, CELLS_COLUMN, COLUMN_GAP, PERIOD_COLUMN, PICTURE, Scrolls, caption, dial, heading, icon_button, icons,
        mono, number, panel_title, picture, scrolling, side_panel,
    },
    library::LOOKS_EVERY,
    sim::SimSystems,
    synced::Synced,
    view::Stamp,
};

pub const KEPT_WIDTH: f32 = 396.0;

/// The sorts, in the order of their panels.
const SHELVES: [Sort; 3] = Sort::ALL;

/// Which of the panels a sort has, if it has one.
fn shelf(sort: Sort) -> Option<usize> {
    SHELVES.iter().position(|shelved| *shelved == sort)
}

#[derive(Resource)]
pub struct Collected {
    /// The collection, and the file it lies in. A scripted run has none, and keeps to itself.
    collection: Synced<Collection>,
    open: [bool; 3],
    /// Counts the changes that show, here or wherever a pattern says whether it is kept.
    revision: u64,
    /// What the panels list, row by row.
    listed: [Vec<Kept>; 3],
}

impl Collected {
    /// The collection of the file at `path`. Without a path it is written nowhere.
    pub fn at(path: Option<PathBuf>) -> Self {
        Self { collection: Synced::at(path), open: [false; 3], revision: 0, listed: Default::default() }
    }

    pub fn toggle(&mut self, sort: Sort) {
        if let Some(shelf) = shelf(sort) {
            self.open[shelf] = !self.open[shelf];
        }
    }

    pub fn is_open(&self, sort: Sort) -> bool {
        shelf(sort).is_some_and(|shelf| self.open[shelf])
    }

    pub fn close(&mut self, sort: Sort) {
        if self.is_open(sort) {
            self.toggle(sort);
        }
    }

    /// What tells a change of what is kept from none.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a pattern of a rule is kept: its cells are those of the form its kind is filed
    /// under.
    pub fn is_kept(&self, rule: &BlockRule, cells: &[Cell]) -> bool {
        self.collection.find(rule, cells).is_some()
    }

    /// The patterns of a sort kept under a rule.
    pub fn of<'a>(&'a self, rule: &'a BlockRule, sort: Sort) -> Vec<&'a Kept> {
        self.collection.of(rule, sort).collect()
    }

    /// Keeps a pattern of a rule, by the form its kind is filed under. True if it was not
    /// kept before.
    pub fn keep(&mut self, rule: &BlockRule, sort: Sort, cells: &[Cell], period: u32, moves: (i32, i32)) -> bool {
        let kept = Kept::new(rule, sort, cells, period, moves);
        let new = self.collection.edit(|collection| collection.keep(kept).is_ok());
        self.revision += 1;
        new
    }

    /// Lets go of a pattern. True if it was kept.
    pub fn forget(&mut self, rule: &BlockRule, cells: &[Cell]) -> bool {
        let was = self.collection.edit(|collection| collection.forget(rule, cells));
        self.revision += 1;
        was
    }

    /// Keeps a pattern that is not kept, and lets go of one that is. True if it is kept now.
    pub fn keep_or_forget(
        &mut self,
        rule: &BlockRule,
        sort: Sort,
        cells: &[Cell],
        period: u32,
        moves: (i32, i32),
    ) -> bool {
        if self.is_kept(rule, cells) {
            self.forget(rule, cells);
            false
        } else {
            self.keep(rule, sort, cells, period, moves);
            true
        }
    }

    /// What is wrong with the file, for as long as it is.
    pub fn trouble(&self) -> Option<&str> {
        self.collection.trouble()
    }
}

/// A panel, the list in it and the line under the list: of which shelf.
#[derive(Component, Default, Clone, Copy)]
struct KeptPanel(usize);

#[derive(Component, Default, Clone, Copy)]
struct KeptList(usize);

#[derive(Component, Default, Clone, Copy)]
struct KeptNote(usize);

/// A row of a list, and its two buttons: the shelf, and which of its patterns.
#[derive(Component, Default, Clone, Copy)]
struct KeptRow(usize, usize);

#[derive(Component, Default, Clone, Copy)]
struct KeptLook(usize, usize);

#[derive(Component, Default, Clone, Copy)]
struct KeptForget(usize, usize);

pub struct KeptPlugin;

impl Plugin for KeptPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (watch_file, show_panels, list_kept, light_rows).chain().in_set(SimSystems::Present));
    }
}

/// The side of the small buttons of a row.
const BUTTON: f32 = 24.0;

/// The button that closes a panel: of which shelf.
#[derive(Component, Default, Clone, Copy)]
struct KeptClose(usize);

pub fn spaceships_panel() -> impl Scene {
    let about = "The spaceships kept under this rule: patterns that are back in their shape, somewhere else, \
                 after their period.";
    kept_panel(0, "Spaceships", about)
}

pub fn oscillators_panel() -> impl Scene {
    let about = "The oscillators kept under this rule: patterns that are back in their shape, where they were, \
                 after their period.";
    kept_panel(1, "Oscillators", about)
}

pub fn still_lifes_panel() -> impl Scene {
    kept_panel(2, "Still lifes", "The still lifes kept under this rule: patterns that stay as they are.")
}

/// The panel of a shelf: the three are alike but for their names, by which the rig knows
/// them, and for what their columns say.
fn kept_panel(shelf: usize, title: &'static str, about: &'static str) -> impl Scene {
    let stem = ["KeptSpaceships", "KeptOscillators", "KeptStillLifes"][shelf];
    let (panel, close, note) = (Name::new(stem), Name::new(format!("{stem}Close")), Name::new(format!("{stem}Note")));
    let list = Name::new(["KeptSpaceshipList", "KeptOscillatorList", "KeptStillLifeList"][shelf]);
    let (this, closes, noted, listed) = (KeptPanel(shelf), KeptClose(shelf), KeptNote(shelf), KeptList(shelf));
    let (how, period) = match SHELVES[shelf] {
        Sort::Spaceship => ("SPEED", "PERIOD"),
        Sort::Oscillator => ("SIZE", "PERIOD"),
        Sort::StillLife => ("SIZE", ""),
    };
    bsn! {
        side_panel(KEPT_WIDTH, bsn_list![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                }
                Children [
                    panel_title(Aspect::Pattern, title),
                    (
                        @FeathersButton {
                            @caption: bsn! { Text("Close") ThemedText }
                        }
                        template_value(close)
                        template_value(closes)
                        on(|activate: On<Activate>, buttons: Query<&KeptClose>, mut collected: ResMut<Collected>| {
                            if let Ok(&KeptClose(shelf)) = buttons.get(activate.entity) {
                                collected.close(SHELVES[shelf]);
                            }
                        })
                    ),
                ]
            ),
            caption(about),
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
                    (Node { flex_grow: 1.0, flex_basis: px(0) } Children [ heading(how) ]),
                    (Node { width: px(PERIOD_COLUMN), justify_content: JustifyContent::End } Children [ heading(period) ]),
                    (Node { width: px(CELLS_COLUMN), justify_content: JustifyContent::End } Children [ heading("CELLS") ]),
                    (Node { width: px(2.0 * BUTTON + COLUMN_GAP) }),
                ]
            ),
            scrolling(Scrolls::Rows, bsn! { template_value(list) template_value(listed) }),
            (caption("") template_value(note) template_value(noted)),
        ])
        template_value(panel)
        template_value(this)
    }
}

/// A kept pattern: its picture; how fast it flies and which way, or how large it is; a word
/// on it; its period if it has one to speak of; its cells; and two small buttons: a closer
/// look, and the mark that it is kept, which lets go of it.
fn kept_row(shelf: usize, index: usize, kept: &Kept) -> impl Scene {
    let stem = ["KeptSpaceship", "KeptOscillator", "KeptStillLife"][shelf];
    let name = Name::new(format!("{stem}{index}"));
    let (look_name, forget_name) = (Name::new(format!("{stem}Look{index}")), Name::new(format!("{stem}Forget{index}")));
    let (row, look, forget) = (KeptRow(shelf, index), KeptLook(shelf, index), KeptForget(shelf, index));
    let span = |axis: fn(&Cell) -> i32| {
        let along = kept.cells.iter().map(axis);
        along.clone().max().unwrap_or(0) - along.min().unwrap_or(0) + 1
    };
    let size = format!("{}×{}", span(|cell| cell.0), span(|cell| cell.1));
    // What was written about it, if anything was.
    let written = if kept.name.is_empty() { kept.note.clone() } else { kept.name.clone() };
    // A spaceship by its speed, with the way the form it is filed under flies on the dial;
    // what stays where it is by its size.
    let motion = Motion { period: kept.period, displacement: kept.moves, canonical: Vec::new() };
    let mut ways = [0; 8];
    let (title, word, period) = match kept.sort {
        Sort::Spaceship => {
            if let Some(way) = way(kept.moves.0, kept.moves.1) {
                ways[way] = 1;
            }
            let word = if written.is_empty() { analysis::heading(&motion).to_string() } else { written };
            (analysis::speed(&motion), word, kept.period.to_string())
        }
        Sort::Oscillator => (size, written, kept.period.to_string()),
        Sort::StillLife => (size, written, String::new()),
    };
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(COLUMN_GAP),
            padding: UiRect::axes(px(7), px(5)),
            border: px(1),
            border_radius: px(5),
            flex_shrink: 0.0,
        }
        BackgroundColor(palette::GRAY_2)
        BorderColor::all(Color::NONE)
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        template_value(name)
        template_value(row)
        on(pick_kept)
        Children [
            picture(&kept.cells),
            (
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
                    mono(title, 14.0, palette::WHITE),
                    (caption(word) template_value(Pickable::IGNORE)),
                ]
            ),
            dial(&ways, None),
            number(period, PERIOD_COLUMN, palette::LIGHT_GRAY_1),
            number(kept.cells.len().to_string(), CELLS_COLUMN, palette::LIGHT_GRAY_1),
            (
                icon_button(icons::LOOK, palette::LIGHT_GRAY_1)
                template_value(look_name)
                template_value(look)
                on(look_at_kept)
            ),
            (
                icon_button(icons::KEEP, Aspect::Pattern.color())
                template_value(forget_name)
                template_value(forget)
                on(forget_kept)
            ),
        ]
    }
}

/// A click on a row picks the pattern up, to be put down on the grid.
fn pick_kept(
    click: On<Pointer<Click>>,
    rows: Query<&KeptRow>,
    universe: Res<Universe>,
    collected: Res<Collected>,
    mut stamp: ResMut<Stamp>,
    mut analysis: ResMut<Analysis>,
) {
    let Ok(&KeptRow(shelf, index)) = rows.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    if let Some(kept) = collected.listed[shelf].get(index) {
        stamp.pick_up(Analyser::new(universe.rule()).forms(&kept.cells), universe.rule(), None);
        // A click on the grid puts the pattern down now, rather than starting a band.
        analysis.stop_choosing();
    }
}

/// The small button with the glass sends the pattern to the analysis panel. The click goes
/// no further: the row would pick the pattern up.
fn look_at_kept(
    mut click: On<Pointer<Click>>,
    buttons: Query<&KeptLook>,
    universe: Res<Universe>,
    collected: Res<Collected>,
    mut analysis: ResMut<Analysis>,
) {
    let Ok(&KeptLook(shelf, index)) = buttons.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if click.button == PointerButton::Primary
        && let Some(kept) = collected.listed[shelf].get(index)
    {
        // A kind is filed at the start of the vacuum's cycle.
        analysis.study(kept.cells.clone(), 0, &universe);
    }
}

/// The mark that a pattern is kept lets go of it.
fn forget_kept(mut click: On<Pointer<Click>>, buttons: Query<&KeptForget>, mut collected: ResMut<Collected>) {
    let Ok(&KeptForget(shelf, index)) = buttons.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if click.button == PointerButton::Primary
        && let Some(kept) = collected.listed[shelf].get(index).cloned()
    {
        collected.forget(&kept.rule, &kept.cells);
    }
}

/// Looks at the file every now and then, and reads it again if something else wrote it.
fn watch_file(mut collected: ResMut<Collected>, time: Res<Time>, mut since: Local<f32>) {
    *since += time.delta_secs();
    if *since < LOOKS_EVERY {
        return;
    }
    *since = 0.0;
    let collected = collected.bypass_change_detection();
    if collected.collection.read_again() {
        collected.revision += 1;
    }
}

/// Shows or hides the panels.
fn show_panels(collected: Res<Collected>, mut panels: Query<(&KeptPanel, &mut Node)>) {
    for (&KeptPanel(shelf), mut panel) in &mut panels {
        let display = if collected.open[shelf] { Display::Flex } else { Display::None };
        if panel.display != display {
            panel.display = display;
        }
    }
}

/// Lists what is kept under the rule on the grid, sort by sort, the latest first.
fn list_kept(
    mut collected: ResMut<Collected>,
    universe: Res<Universe>,
    lists: Query<(Entity, &KeptList)>,
    mut notes: Query<(&KeptNote, &mut Text)>,
    mut shown: Local<Option<(u64, BlockRule)>>,
    mut commands: Commands,
) {
    let now = (collected.revision, universe.rule().clone());
    if shown.as_ref() == Some(&now) {
        return;
    }
    *shown = Some(now);
    let rule = universe.rule();
    let listed: [Vec<Kept>; 3] = SHELVES.map(|sort| collected.of(rule, sort).into_iter().rev().cloned().collect());
    for (list, &KeptList(shelf)) in &lists {
        let rows: Vec<Entity> = listed[shelf]
            .iter()
            .enumerate()
            .map(|(index, kept)| commands.spawn_scene(kept_row(shelf, index, kept)).id())
            .collect();
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).add_children(&rows);
    }
    for (&KeptNote(shelf), mut text) in &mut notes {
        let said = match (collected.trouble(), listed[shelf].is_empty()) {
            (Some(trouble), _) => trouble.to_string(),
            (None, true) => "None kept under this rule yet. Keep puts a pattern here.".to_string(),
            (None, false) => "Click a pattern to pick it up. Its mark lets go of it.".to_string(),
        };
        text.set_if_neq(Text(said));
    }
    collected.bypass_change_detection().listed = listed;
}

/// A row lights up under the pointer, since a click on it does something.
fn light_rows(mut rows: Query<(&Hovered, &mut BackgroundColor), With<KeptRow>>) {
    for (hovered, mut background) in &mut rows {
        let color = if hovered.0 { palette::GRAY_3 } else { palette::GRAY_2 };
        if background.0 != color {
            background.0 = color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_kept_is_in_the_file_at_once() {
        let folder = std::env::temp_dir().join(format!("cas-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("patterns.tsv");
        let rule: BlockRule = "single-rotation".parse().unwrap();
        let (ship, block) = (vec![(1, 0), (2, 0), (1, 2), (2, 2)], vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
        let mut collected = Collected::at(Some(path.clone()));
        assert!(!path.exists() && !collected.is_kept(&rule, &ship));
        // Kept once, however often it is asked for; the same cells two blocks over are the
        // same pattern.
        assert!(collected.keep(&rule, Sort::Spaceship, &ship, 12, (2, 0)));
        assert!(!collected.keep(&rule, Sort::Spaceship, &ship, 12, (2, 0)));
        let moved: Vec<Cell> = ship.iter().map(|&(x, y)| (x + 2, y + 4)).collect();
        assert!(collected.is_kept(&rule, &moved));
        assert!(collected.keep_or_forget(&rule, Sort::StillLife, &block, 1, (0, 0)));
        let on_disk = Collection::read(&path).unwrap();
        assert_eq!(on_disk.all().len(), 2);
        assert_eq!((collected.of(&rule, Sort::StillLife).len(), collected.of(&rule, Sort::Oscillator).len()), (1, 0));
        // What something else puts in the file is not written over by the next change here.
        let mut outside = on_disk.clone();
        outside.keep(Kept::new(&rule, Sort::Oscillator, &[(0, 0)], 4, (0, 0))).unwrap();
        outside.write(&path).unwrap();
        assert!(!collected.keep_or_forget(&rule, Sort::StillLife, &block, 1, (0, 0)));
        let on_disk = Collection::read(&path).unwrap();
        assert_eq!(on_disk.all().len(), 2);
        assert!(collected.is_kept(&rule, &[(0, 0)]) && !collected.is_kept(&rule, &block));
        // Each sort has a panel of its own.
        collected.toggle(Sort::Oscillator);
        assert!(
            collected.is_open(Sort::Oscillator)
                && !collected.is_open(Sort::StillLife)
                && !collected.is_open(Sort::Spaceship)
        );
        let _ = std::fs::remove_dir_all(&folder);
    }
}
