//! The patterns kept under a rule: the spaceships, the oscillators and the still lifes, each
//! sort in a panel of its own.
//!
//! What is kept lies in a folder next to the library's file, a file to a rule
//! ([`cas_core::collection`]). A rule's file is read on another thread when the rule comes on
//! the grid, and again whenever something else wrote it; it is written when something is
//! kept or let go of, on top of what it has then. Nothing gets there by itself: a pattern is
//! kept with a button, in the analysis panel or in the list of what was caught, or by a
//! search that was asked to.
//!
//! Thousands of patterns may be kept under a rule, and a row of a list is many nodes: a list
//! has rows only for what is in sight, and room above and below them for the rest, as much
//! as their rows would take, so that it scrolls as if they were all there.

use std::{
    cmp::Ordering,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::SystemTime,
};

use bevy::{feathers::palette, picking::hover::Hovered, prelude::*, text::LineBreak, ui_widgets::Activate};

use cas_core::{
    collection::{Collection, Kept, Sort},
    pattern::{Analyser, Cell, Heading, Motion},
    rules::BlockRule,
    universe::Universe,
};
use cas_ui::{
    Aspect, CELLS_COLUMN, COLUMN_GAP, GLYPH, PERIOD_COLUMN, PICTURE, Scrolls, caption, dial, heading, icon_button,
    icons, list_row, mono, number, panel_header, panel_title, picture, scrolling, side_panel, tile, tile_label,
    tile_picture, tile_value,
};

use crate::{
    analysis::{self, Analysis},
    library::LOOKS_EVERY,
    sim::SimSystems,
    view::Stamp,
};

pub const KEPT_WIDTH: f32 = 396.0;

/// The sorts, in the order of their panels.
const SHELVES: [Sort; 3] = Sort::ALL;

/// Which of the panels a sort has, if it has one.
fn shelf(sort: Sort) -> Option<usize> {
    SHELVES.iter().position(|shelved| *shelved == sort)
}

/// A file as it was when last read or written here: when it was written, and how long it is.
/// None for a file that is not there.
type Seen = Option<(SystemTime, u64)>;

fn seen(path: &Path) -> Seen {
    let found = fs::metadata(path).ok()?;
    Some((found.modified().ok()?, found.len()))
}

/// What a reading of a rule's file brings back: which reading it was, of which rule, the file
/// as it was before it was read, and its patterns, or what is wrong with it.
struct Reading {
    asked: u64,
    rule: BlockRule,
    seen: Seen,
    result: Result<Collection, String>,
}

/// The rows a list has: so many of its patterns, from the first.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Shown {
    first: usize,
    count: usize,
}

#[derive(Resource)]
pub struct Collected {
    /// The folder the files are in. A scripted run has none, and keeps to itself.
    folder: Option<PathBuf>,
    /// The patterns of the rules whose files were read, and of those something was kept
    /// under.
    collection: Collection,
    /// The files as they were when read or written here, by rule.
    seen: HashMap<BlockRule, Seen>,
    /// The rules whose files are being read on other threads, by the number of the reading:
    /// only the answer to the latest reading of a rule counts.
    readings: HashMap<BlockRule, u64>,
    asked: u64,
    answers: Mutex<Receiver<Reading>>,
    reply: Sender<Reading>,
    /// Why a rule's file could not be read, by rule, for as long as it cannot: nothing is
    /// kept under the rule until that is put right. And why a file could not be written,
    /// the last time one was to be.
    unreadable: HashMap<BlockRule, String>,
    unwritten: Option<String>,
    open: [bool; 3],
    /// Counts the changes that show, here or wherever a pattern says whether it is kept.
    revision: u64,
    /// What the panels list, row by row.
    listed: [Vec<Kept>; 3],
    /// The rows each list has of what it lists.
    shown: [Shown; 3],
}

impl Collected {
    /// The collection of the folder at `path`. Without a path it is written nowhere.
    pub fn at(path: Option<PathBuf>) -> Self {
        let (reply, answers) = channel();
        Self {
            folder: path,
            collection: Collection::default(),
            seen: HashMap::new(),
            readings: HashMap::new(),
            asked: 0,
            answers: Mutex::new(answers),
            reply,
            unreadable: HashMap::new(),
            unwritten: None,
            open: [false; 3],
            revision: 0,
            listed: Default::default(),
            shown: [Shown::default(); 3],
        }
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
    /// under. Of a rule whose file was not read yet, nothing is.
    pub fn is_kept(&self, rule: &BlockRule, cells: &[Cell]) -> bool {
        self.collection.is_kept(rule, cells)
    }

    /// The patterns of a sort kept under a rule.
    pub fn of<'a>(&'a self, rule: &'a BlockRule, sort: Sort) -> Vec<&'a Kept> {
        self.collection.of(rule, sort).collect()
    }

    /// Keeps a pattern of a rule, by the form its kind is filed under. True if it was not
    /// kept before.
    pub fn keep(&mut self, rule: &BlockRule, sort: Sort, cells: &[Cell], period: u32, moves: (i32, i32)) -> bool {
        let kept = Kept::new(rule, sort, cells, period, moves);
        self.edit(rule, |collection| collection.keep(kept).is_ok())
    }

    /// Lets go of a pattern. True if it was kept.
    pub fn forget(&mut self, rule: &BlockRule, cells: &[Cell]) -> bool {
        self.edit(rule, |collection| collection.forget(rule, cells))
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
        } else {
            self.keep(rule, sort, cells, period, moves);
        }
        self.is_kept(rule, cells)
    }

    /// What is wrong with a rule's file, for as long as it is: it cannot be read, or the
    /// last file to be written could not be.
    pub fn trouble(&self, rule: &BlockRule) -> Option<&str> {
        self.unreadable.get(rule).map(String::as_str).or(self.unwritten.as_deref())
    }

    /// Whether what is kept under a rule is known here: its file was read, or there is no
    /// folder to read from.
    fn loaded(&self, rule: &BlockRule) -> bool {
        self.folder.is_none() || self.seen.contains_key(rule)
    }

    /// Whether a rule's file is being read on another thread.
    fn reading(&self, rule: &BlockRule) -> bool {
        self.readings.contains_key(rule)
    }

    /// Has a rule's file read on another thread: when the rule comes on the grid, and when
    /// something else wrote the file since.
    fn ask(&mut self, rule: &BlockRule) {
        let Some(folder) = &self.folder else {
            return;
        };
        self.asked += 1;
        self.readings.insert(rule.clone(), self.asked);
        let (asked, reply, path, rule) = (self.asked, self.reply.clone(), Collection::file(folder, rule), rule.clone());
        std::thread::spawn(move || {
            // The file as it is before it is read: written meanwhile, it is another file at
            // the next look.
            let seen = seen(&path);
            let result = Collection::read_file(&path, &rule);
            // Nobody listens if the app has gone.
            let _ = reply.send(Reading { asked, rule, seen, result });
        });
    }

    /// Takes in what the readings brought back: the answer to the latest reading of a rule.
    /// True if any did.
    fn hear(&mut self) -> bool {
        let answers: Vec<Reading> =
            self.answers.lock().map_or_else(|_| Vec::new(), |answers| answers.try_iter().collect());
        let mut heard = false;
        for reading in answers {
            if self.readings.get(&reading.rule) != Some(&reading.asked) {
                continue;
            }
            self.readings.remove(&reading.rule);
            self.took(reading);
            heard = true;
        }
        heard
    }

    /// What a reading of a rule's file brought back, in place of what was held of the rule.
    /// A file that cannot be read changes nothing, and is said to be wrong for as long as
    /// it is.
    fn took(&mut self, reading: Reading) {
        let Reading { rule, seen, result, .. } = reading;
        self.seen.insert(rule.clone(), seen);
        match result {
            Ok(read) => {
                self.collection.take(&rule, read);
                self.unreadable.remove(&rule);
            }
            Err(error) => {
                let file = self.folder.as_ref().map(|folder| Collection::file(folder, &rule)).unwrap_or_default();
                let said =
                    format!("{}: {error}. Nothing is kept under this rule until that is put right.", file.display());
                self.unreadable.insert(rule, said);
            }
        }
        self.revision += 1;
    }

    /// Reads a rule's file now, unless it is known here as it is: before anything is kept
    /// under the rule, so that nothing the file has is lost. A reading of it on its way is
    /// of no use then.
    fn read_now(&mut self, rule: &BlockRule) {
        let Some(folder) = &self.folder else {
            return;
        };
        let path = Collection::file(folder, rule);
        let seen = seen(&path);
        if self.seen.get(rule) == Some(&seen) {
            return;
        }
        self.asked += 1;
        self.readings.remove(rule);
        let result = Collection::read_file(&path, rule);
        self.took(Reading { asked: self.asked, rule: rule.clone(), seen, result });
    }

    /// Changes what is kept under a rule, in its file as well, if `change` says that it did
    /// change something: then this is true. What is changed is what the file has now:
    /// anything that something else wrote there meanwhile is read first, and so is not lost.
    /// Under a rule whose file cannot be read nothing is changed.
    fn edit(&mut self, rule: &BlockRule, change: impl FnOnce(&mut Collection) -> bool) -> bool {
        self.read_now(rule);
        if self.unreadable.contains_key(rule) || !change(&mut self.collection) {
            return false;
        }
        self.revision += 1;
        if let Some(folder) = self.folder.clone() {
            let path = Collection::file(&folder, rule);
            let failed = |error: std::io::Error| format!("{} could not be written: {error}.", path.display());
            self.unwritten = self.collection.write(&folder).err().map(failed);
            self.seen.insert(rule.clone(), seen(&path));
        }
        true
    }
}

/// A panel, the list in it and the line under the list: of which shelf.
#[derive(Component, Default, Clone, Copy)]
struct KeptPanel(usize);

#[derive(Component, Default, Clone, Copy)]
struct KeptList(usize);

#[derive(Component, Default, Clone, Copy)]
struct KeptNote(usize);

/// What the kept spaceships come to, over their list: the tile of it, the box its dial is
/// in, and the line that says how they fly.
#[derive(Component, Default, Clone, Copy)]
struct KeptWays;

#[derive(Component, Default, Clone, Copy)]
struct KeptWaysDial;

#[derive(Component, Default, Clone, Copy)]
struct KeptWaysSaid;

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
        app.add_systems(
            Update,
            (follow_rule, hear_readings, show_panels, list_kept, scroll_kept, say_what_next)
                .chain()
                .in_set(SimSystems::Present),
        );
    }
}

/// The side of the small buttons of a row.
const BUTTON: f32 = 24.0;
/// How high a row is, until one is there to be measured.
const ROW_HEIGHT: f32 = 56.0;
/// A list has rows for what is in sight and so many more either side, and makes them anew
/// when the sight comes within so many of their end.
const MARGIN: usize = 16;
const SLACK: usize = 6;

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
    // Only what flies goes any way.
    let ways: Vec<_> = (SHELVES[shelf] == Sort::Spaceship).then(ways_tile).into_iter().collect();
    bsn! {
        side_panel(KEPT_WIDTH, bsn_list![
            panel_header(panel_title(Aspect::Pattern, title), bsn! {
                template_value(close)
                template_value(closes)
                on(|activate: On<Activate>, buttons: Query<&KeptClose>, mut collected: ResMut<Collected>| {
                    if let Ok(&KeptClose(shelf)) = buttons.get(activate.entity) {
                        collected.close(SHELVES[shelf]);
                    }
                })
            }),
            caption(about),
            { ways },
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

/// The ways the kept spaceships of the rule go, all of them together: a dial with the ways
/// lit that one kind of them at least can go, and in words how many kinds fly straight, along
/// a diagonal, or neither. It says what was found in the rule's world, not what there is. Not
/// there while no spaceship is kept.
fn ways_tile() -> impl Scene {
    bsn! {
        tile()
        Node { display: Display::None }
        #KeptSpaceshipsWays
        KeptWays
        Children [
            (tile_picture(GLYPH) KeptWaysDial),
            (
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_width: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                }
                Children [
                    tile_label("WAYS"),
                    (tile_value("") KeptWaysSaid),
                ]
            ),
        ]
    }
}

/// How many kinds of ship fly straight, along a diagonal, or neither, in words.
fn headings(ships: &[Kept]) -> String {
    let flies = |kept: &Kept| Motion { period: kept.period, displacement: kept.moves, canonical: Vec::new() }.heading();
    let count = |heading: Heading| ships.iter().filter(|kept| flies(kept) == heading).count();
    let said = [(Heading::Orthogonal, "orthogonal"), (Heading::Diagonal, "diagonal"), (Heading::Oblique, "oblique")];
    let said = said.into_iter().map(|(heading, name)| (count(heading), name)).filter(|&(count, _)| count > 0);
    said.map(|(count, name)| format!("{count} {name}")).collect::<Vec<_>>().join(" · ")
}

/// A kept pattern: its picture; how fast it flies, or how large it is; a word on it; the
/// ways it can go, which are those `possible` for it, lit on its dial; its period if it has
/// one to speak of; its cells; and two small buttons: a closer look, and the mark that it is
/// kept, which lets go of it. Every row is one line high, whatever is written on it, so that
/// the room for the rows not made is as much as they would take. One that takes the place
/// of the row under the pointer is `lit` from the start.
fn kept_row(shelf: usize, index: usize, kept: &Kept, possible: &[bool; 8], lit: bool) -> impl Scene {
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
    // A spaceship by its speed; what stays where it is by its size.
    let motion = Motion { period: kept.period, displacement: kept.moves, canonical: Vec::new() };
    // On the dial a way is lit or it is not: lit, every way the pattern can go, each as much
    // as the other. The list is of what is known, and nothing in it was counted.
    let ways = possible.map(u64::from);
    let period = period_of(kept).map_or(String::new(), |period| period.to_string());
    let (title, word) = match kept.sort {
        Sort::Spaceship => {
            // Straight, along a diagonal or neither is said of every ship: the dial has only
            // eight ways, and a ship that flies between two of them is on the diagonal.
            let heading = analysis::heading(&motion);
            let word = if written.is_empty() { heading.to_string() } else { format!("{heading} · {written}") };
            (analysis::speed(&motion), word)
        }
        Sort::Oscillator | Sort::StillLife => (size, written),
    };
    bsn! {
        list_row()
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(COLUMN_GAP),
        }
        Hovered(lit)
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
                    (mono(title, 14.0, palette::WHITE) TextLayout { linebreak: LineBreak::NoWrap }),
                    (
                        caption(word)
                        TextLayout { linebreak: LineBreak::NoWrap }
                        template_value(Pickable::IGNORE)
                    ),
                ]
            ),
            dial(&ways, possible, None),
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
        stamp.pick_up(&kept.cells, kept.moves, universe.rule(), None);
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

/// The rule on the grid has its file read when it comes on, if it was not read yet; and
/// every now and then the file is looked at, for what something else wrote there, and read
/// again then.
fn follow_rule(mut collected: ResMut<Collected>, universe: Res<Universe>, time: Res<Time>, mut since: Local<f32>) {
    let collected = collected.bypass_change_detection();
    let rule = universe.rule();
    if !collected.loaded(rule) && !collected.reading(rule) {
        collected.ask(rule);
    }
    *since += time.delta_secs();
    if *since < LOOKS_EVERY {
        return;
    }
    *since = 0.0;
    if let Some(folder) = &collected.folder
        && collected.loaded(rule)
        && !collected.reading(rule)
        && collected.seen.get(rule) != Some(&seen(&Collection::file(folder, rule)))
    {
        collected.ask(rule);
    }
}

/// Takes in what the readings brought back.
fn hear_readings(mut collected: ResMut<Collected>) {
    if collected.bypass_change_detection().hear() {
        collected.set_changed();
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

/// The period of a kept pattern, if it has one to speak of. What stands still has none: the
/// one it was kept with is the length of its vacuum's cycle, or simply 1.
fn period_of(kept: &Kept) -> Option<u32> {
    (kept.sort != Sort::StillLife).then_some(kept.period)
}

/// The order of a list: the faster ship first; of two as fast, the one of the shorter period;
/// of two of one period, the one of fewer cells. What stays where it is has no speed, and
/// goes by its period and its cells; what stands still has no period either
/// ([`period_of`]), and goes by its cells.
fn in_order(a: &Kept, b: &Kept) -> Ordering {
    // Cells per generation along the faster axis, as a speed is given: compared without
    // dividing.
    let far = |kept: &Kept| u64::from(kept.moves.0.unsigned_abs().max(kept.moves.1.unsigned_abs()));
    let faster = (far(b) * u64::from(a.period)).cmp(&(far(a) * u64::from(b.period)));
    faster.then(period_of(a).cmp(&period_of(b))).then(a.cells.len().cmp(&b.cells.len()))
}

/// Lists what is kept under the rule on the grid, sort by sort and in order ([`in_order`]):
/// of two that are alike in all it goes by, the one kept last comes first. The rows are
/// made as the lists are looked at ([`scroll_kept`]); the lists of another rule begin at
/// their top.
fn list_kept(
    mut collected: ResMut<Collected>,
    universe: Res<Universe>,
    mut lists: Query<(Entity, &mut ScrollPosition), With<KeptList>>,
    mut ways_tiles: Query<&mut Node, With<KeptWays>>,
    ways_dials: Query<Entity, With<KeptWaysDial>>,
    mut ways_said: Query<&mut Text, With<KeptWaysSaid>>,
    mut shown: Local<Option<(u64, BlockRule)>>,
    mut commands: Commands,
) {
    let now = (collected.revision, universe.rule().clone());
    if shown.as_ref() == Some(&now) {
        return;
    }
    let other_rule = shown.as_ref().is_none_or(|(_, rule)| *rule != now.1);
    *shown = Some(now);
    let rule = universe.rule();
    let listed: [Vec<Kept>; 3] = SHELVES.map(|sort| {
        let mut kept: Vec<Kept> = collected.of(rule, sort).into_iter().rev().cloned().collect();
        kept.sort_by(in_order);
        kept
    });
    for (list, mut scroll) in &mut lists {
        commands.entity(list).despawn_related::<Children>();
        if other_rule {
            scroll.y = 0.0;
        }
    }
    // A ship can go every way that the rule's world looks the same.
    let analyser = Analyser::new(rule);
    // Over the spaceships, the ways they go between them: a way is lit if one kind of them
    // can go it, and as much as any other, however many kinds can.
    let ships = shelf(Sort::Spaceship).map_or(&[][..], |shelf| &listed[shelf][..]);
    let mut ways = [false; 8];
    for kept in ships {
        for (way, possible) in analyser.ways(kept.moves).into_iter().enumerate() {
            ways[way] |= possible;
        }
    }
    let display = if ships.is_empty() { Display::None } else { Display::Flex };
    for mut tile in &mut ways_tiles {
        if tile.display != display {
            tile.display = display;
        }
    }
    for picture in &ways_dials {
        let dial = commands.spawn_scene(dial(&ways.map(u64::from), &ways, None)).id();
        commands.entity(picture).despawn_related::<Children>();
        commands.entity(picture).add_child(dial);
    }
    for mut said in &mut ways_said {
        said.set_if_neq(Text(headings(ships)));
    }
    let collected = collected.bypass_change_detection();
    collected.listed = listed;
    collected.shown = [Shown::default(); 3];
}

/// Keeps each list that is open with rows for what is in sight and so many more either side,
/// and room for the rest: the rows are made anew when the sight comes near their end. The
/// pointer is only found on them a frame later: the row in the place of the one it is on is
/// lit from the start, or a click would make it blink.
fn scroll_kept(
    mut collected: ResMut<Collected>,
    universe: Res<Universe>,
    lists: Query<(Entity, &KeptList, &ComputedNode, &ScrollPosition)>,
    rows: Query<(&KeptRow, &ComputedNode, &Hovered)>,
    mut commands: Commands,
) {
    let collected = collected.bypass_change_detection();
    let mut analyser = None;
    let gap = Scrolls::Rows.gap();
    for (list, &KeptList(shelf), computed, scroll) in &lists {
        let count = collected.listed[shelf].len();
        if !collected.open[shelf] || count == 0 {
            continue;
        }
        // A row and the gap under it, as high as the rows there are, until there is one.
        let measured = rows.iter().find(|(row, node, _)| row.0 == shelf && node.size.y > 1.0);
        let pitch = measured.map_or(ROW_HEIGHT, |(_, node, _)| node.size.y * node.inverse_scale_factor) + gap;
        let sight = computed.size.y * computed.inverse_scale_factor;
        let top = scroll.y.max(0.0);
        let (first_in_sight, last_in_sight) =
            ((top / pitch) as usize, (((top + sight) / pitch).ceil() as usize).min(count));
        let Shown { first, count: made } = collected.shown[shelf];
        let end = first + made;
        let short_above = first > 0 && first_in_sight < first + SLACK;
        let short_below = end < count && last_in_sight + SLACK > end;
        if made > 0 && !short_above && !short_below {
            continue;
        }
        let (first, end) = (first_in_sight.saturating_sub(MARGIN), (last_in_sight + MARGIN).min(count));
        let lit = rows.iter().find(|(row, _, hovered)| row.0 == shelf && hovered.0).map(|(row, ..)| row.1);
        let analyser = analyser.get_or_insert_with(|| Analyser::new(universe.rule()));
        let mut children = Vec::new();
        if first > 0 {
            children.push(commands.spawn_scene(room(first as f32 * pitch - gap)).id());
        }
        for index in first..end {
            let kept = &collected.listed[shelf][index];
            let row = kept_row(shelf, index, kept, &analyser.ways(kept.moves), lit == Some(index));
            children.push(commands.spawn_scene(row).id());
        }
        if end < count {
            children.push(commands.spawn_scene(room((count - end) as f32 * pitch - gap)).id());
        }
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).add_children(&children);
        collected.shown[shelf] = Shown { first, count: end - first };
    }
}

/// Room for rows that are not made: as high as they would be.
fn room(height: f32) -> impl Scene {
    bsn! {
        Node { height: px(height), flex_shrink: 0.0 }
    }
}

/// The line under each list: what is wrong with the file, that it is being read, what the
/// list is for, or what there is to do with a pattern that is held.
fn say_what_next(
    collected: Res<Collected>,
    universe: Res<Universe>,
    stamp: Res<Stamp>,
    mut notes: Query<(&KeptNote, &mut Text)>,
) {
    // A stamp from the list of what was caught is that list's to speak of. Of any other,
    // every panel that hands patterns out says the same, wherever it was picked up.
    let holding = stamp.is_held() && stamp.kind.is_none();
    let rule = universe.rule();
    for (&KeptNote(shelf), mut text) in &mut notes {
        let said = match (collected.trouble(rule), collected.listed[shelf].is_empty()) {
            (Some(trouble), _) => trouble,
            (None, true) if collected.reading(rule) => "Reading what is kept under this rule…",
            (None, true) => "None kept under this rule yet. Keep puts a pattern here.",
            (None, false) if holding => stamp.hint(),
            (None, false) => "Click a pattern to pick it up. Its mark lets go of it.",
        };
        if text.0 != said {
            text.0 = said.to_string();
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
        let path = folder.join("patterns");
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
        // What something else puts in the files is not written over by the next change here.
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

    #[test]
    fn a_rule_s_file_is_read_on_another_thread_when_asked_for() {
        let folder = std::env::temp_dir().join(format!("cas-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("patterns");
        let rule: BlockRule = "single-rotation".parse().unwrap();
        let (ship, block) = (vec![(1, 0), (2, 0), (1, 2), (2, 2)], vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
        let mut outside = Collection::default();
        outside.keep(Kept::new(&rule, Sort::Spaceship, &ship, 12, (2, 0))).unwrap();
        outside.write(&path).unwrap();
        // Nothing is known of a rule until its file was read, which is asked for and comes
        // back from another thread.
        let mut collected = Collected::at(Some(path.clone()));
        assert!(!collected.loaded(&rule) && !collected.is_kept(&rule, &ship));
        collected.ask(&rule);
        assert!(collected.reading(&rule) && !collected.hear());
        while collected.reading(&rule) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            collected.hear();
        }
        assert!(collected.loaded(&rule) && collected.is_kept(&rule, &ship) && collected.trouble(&rule).is_none());
        // Written from outside since, the file is another, and is read before anything is
        // kept under the rule: nothing it has is lost.
        outside.keep(Kept::new(&rule, Sort::Oscillator, &[(0, 0)], 4, (0, 0))).unwrap();
        outside.write(&path).unwrap();
        assert_ne!(collected.seen[&rule], seen(&Collection::file(&path, &rule)));
        assert!(collected.keep(&rule, Sort::StillLife, &block, 1, (0, 0)));
        assert!(collected.is_kept(&rule, &[(0, 0)]) && collected.is_kept(&rule, &block));
        assert_eq!(Collection::read(&path).unwrap().all().len(), 3);
        assert_eq!(collected.seen[&rule], seen(&Collection::file(&path, &rule)));
        // A file that cannot be read is said to be wrong, and nothing is kept under its rule
        // until it is put right; the other rules are not affected.
        std::fs::write(Collection::file(&path, &rule), "oscillator\to\tsoon\n").unwrap();
        collected.ask(&rule);
        while collected.reading(&rule) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            collected.hear();
        }
        assert!(collected.trouble(&rule).is_some_and(|said| said.contains("line 1")));
        assert!(!collected.keep(&rule, Sort::Oscillator, &[(0, 0), (1, 1)], 2, (0, 0)));
        assert!(collected.is_kept(&rule, &block));
        let critters: BlockRule = "critters".parse().unwrap();
        assert!(
            collected.keep(&critters, Sort::StillLife, &block, 1, (0, 0)) && collected.trouble(&critters).is_none()
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_lists_are_in_order() {
        let rule: BlockRule = "single-rotation".parse().unwrap();
        let cells = |count: i32| (0..count).map(|x| (x, 0)).collect::<Vec<Cell>>();
        let ship = |count, period, moves| Kept::new(&rule, Sort::Spaceship, &cells(count), period, moves);
        // By speed, which is along the faster axis: 2c/5, c/3, c/6 three times and 2c/15.
        let (fast, slow) = (ship(9, 3, (-1, 0)), ship(2, 30, (4, 2)));
        let (short, long, light) = (ship(7, 12, (0, 2)), ship(3, 24, (4, 4)), ship(5, 12, (-2, 0)));
        let fastest = ship(8, 5, (1, -2));
        let mut ships = vec![slow.clone(), long.clone(), short.clone(), fast.clone(), light.clone(), fastest.clone()];
        ships.sort_by(in_order);
        assert_eq!(ships, [fastest, fast, light, short, long, slow]);
        // What stays where it is: by period, then by cells.
        let stays = |count, period| Kept::new(&rule, Sort::Oscillator, &cells(count), period, (0, 0));
        let mut oscillators = vec![stays(4, 8), stays(6, 2), stays(1, 4), stays(3, 2), stays(2, 4)];
        oscillators.sort_by(in_order);
        assert_eq!(oscillators, [stays(3, 2), stays(6, 2), stays(1, 4), stays(2, 4), stays(4, 8)]);
        // What stands still: by cells alone, whatever period it was kept with.
        let stands = |count, period| Kept::new(&rule, Sort::StillLife, &cells(count), period, (0, 0));
        let mut still = vec![stands(4, 1), stands(3, 2), stands(6, 1)];
        still.sort_by(in_order);
        assert_eq!(still, [stands(3, 2), stands(4, 1), stands(6, 1)]);
    }
}
