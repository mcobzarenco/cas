//! Finite patterns on the unbounded plane, and what they do when left alone.
//!
//! A pattern is a list of cells that differ from the vacuum. In a block automaton it is placed
//! not only by its shape but by how it sits on the block grid, so its coordinates are always
//! relative to a corner of the blocks the next step rewrites: those blocks have even corners.
//!
//! An [`Analyser`] runs a pattern in empty space until it is back in its starting shape. That
//! gives its period and how far it moved, and the form to file it under: one and the same
//! whatever phase and orientation it was found in. It also tells a single pattern from several
//! that merely travel together.

use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use crate::rules::{BlockRule, anti_transpose, flip, mirror, rotate_180, rotate_ccw, rotate_cw, transpose};

/// `(x, y)`, with `y` pointing down.
pub type Cell = (i32, i32);

/// What a pattern does, left alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Motion {
    /// Generations until it is back in the same shape.
    pub period: u32,
    /// How far it has moved by then. Zero for an oscillator.
    pub displacement: (i32, i32),
    /// The form it is filed under, see [`Analyser::analyse`].
    pub canonical: Vec<Cell>,
}

/// What becomes of a pattern left alone, as far as an [`Analyser`] follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// It is back in its starting shape after `period` generations, `displacement` away.
    Returns { period: u32, displacement: (i32, i32) },
    /// It has more cells than the analyser follows.
    Grows,
    /// It is wider than the analyser follows: its parts have flown apart.
    Scatters,
    /// Neither, for as many generations as the analyser follows.
    Undecided,
}

/// What there is to say about a pattern left alone: its fate, and what was seen on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Study {
    /// The pattern as it set out, settled and at the start of the tables' cycle: a pattern
    /// found later in the cycle is taken there first.
    pub start: Vec<Cell>,
    pub fate: Fate,
    /// After how many generations it is back in its shape, if it is known to be: as it was
    /// seen to, or as its pieces say. Several patterns that never meet, each back where it
    /// was after a period of its own, are all back at once after the least common multiple
    /// of those periods. That is soon more generations than anything is followed for: the
    /// fate of such a pattern is undecided, and it is an oscillator all the same.
    pub period: Option<u128>,
    /// How it moves, if it was followed until it was back in its shape.
    pub motion: Option<Motion>,
    /// Whether every generation finds the same cells in the same places. (Its period is
    /// still that of the tables, two at least where the vacuum flips.)
    pub still: bool,
    /// The fewest and the most cells it had while it was followed.
    pub cells: (usize, usize),
    /// The widest and the highest its bounding box got.
    pub extent: (i32, i32),
    /// How many patterns it is that never meet: one, for a pattern of a piece. Known only for
    /// one that comes back to its shape; 0 otherwise.
    pub parts: usize,
    /// For how many generations it was followed: one known by its pieces, for as long as the
    /// slowest of them takes to be back.
    pub generations: u32,
    /// The turns and mirrors of the square under which the pattern, as it set out, is itself;
    /// and which those are: as it is, turned a quarter clockwise, turned about, turned a
    /// quarter the other way, mirrored left to right and top to bottom, and mirrored across
    /// either diagonal, in the order of [`TURNS_AND_MIRRORS`](crate::rules::TURNS_AND_MIRRORS).
    pub symmetry: Symmetry,
    pub symmetries: [bool; 8],
    /// Whether the pattern is one of its own turns or mirrors (of those the rule allows) before
    /// it is back in its shape, and after how many generations it first is: a glider is its
    /// mirror image half way through its period.
    pub recurs: Option<(u32, Turn)>,
    /// How many cells change from one generation to the next, on average.
    pub heat: f32,
    /// How many cells were alive, where they are, the whole time it was followed.
    pub stator: usize,
    /// For a pattern that did not come back to its shape: the power of time its cells went
    /// with, 2 for one that spreads over the plane, 1 for one that grows along lines, as a gun
    /// does, 0 for one that did not grow to speak of. (A gun's streams are often too wide
    /// before they are too many cells, so its fate is that it flies apart; its growth tells it
    /// from a few ships parting.)
    pub growth: Option<f32>,
    /// For a pattern that does not come back to its shape: what it had become, taken apart
    /// into the pieces that go their own ways, the largest first, each followed on its own;
    /// and how many pieces beyond those were not looked at. For a pattern that comes back as
    /// several that never meet: those. None for a pattern of a piece.
    pub pieces: Vec<Piece>,
    pub more_pieces: usize,
}

/// The turns and mirrors of the square under which a pattern is itself, as it sits on the
/// block grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symmetry {
    None,
    /// One mirror, left to right or top to bottom.
    Mirror,
    /// One mirror, across a diagonal.
    DiagonalMirror,
    HalfTurn,
    /// Left to right and top to bottom, which make a half turn.
    TwoMirrors,
    TwoDiagonalMirrors,
    QuarterTurn,
    /// Every turn and mirror.
    All,
}

/// A turn or a mirror of the square, short of leaving it as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Quarter,
    Half,
    Mirror,
    DiagonalMirror,
}

/// A piece of what a pattern became: cells that go their own way, apart from the rest, and
/// what they do on their own.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub cells: usize,
    pub kind: PieceKind,
    /// The form the piece is filed under, if it comes back to its shape: one and the same
    /// for a ship whichever way it flies and whenever it is met ([`Motion::canonical`]).
    /// Empty for a piece that does not come back.
    pub form: Vec<Cell>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    StillLife,
    Oscillator {
        period: u32,
    },
    Spaceship {
        period: u32,
        displacement: (i32, i32),
    },
    Grows,
    Scatters,
    Undecided,
    /// Too big to follow.
    Unexamined,
}

/// What a pattern became is followed for so many more generations to see which of its cells
/// still meet (long enough to tell apart what is parting, short enough that a gun's own
/// piece is the gun and not its whole stream); the pieces that do not are then followed on
/// their own, the largest first, each for so many generations and so far beyond its own
/// width, until so much work (cells times generations) has been spent on them; a piece with
/// more cells than this is not followed. A pattern is looked at by its pieces before it is
/// followed, too ([`Analyser::apart`]), with as much work allowed.
const PIECE_WINDOW: u32 = 64;
const PIECE_GENERATIONS: u32 = 512;
const PIECE_WORK: u64 = 4_000_000;
const PIECE_EXTENT: i32 = 128;
const PIECE_CELLS: usize = 400;
/// A pattern whose cells go with a higher power of time than this spreads over the plane,
/// and what it spreads into is no pieces; one whose cells go with a lower power than
/// [`GROWING`] is not growing at all.
pub const SPREADING: f32 = 1.5;
pub const GROWING: f32 = 0.5;

/// The directions a pattern can travel in, as far as a square grid tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heading {
    Still,
    Orthogonal,
    Diagonal,
    Oblique,
}

impl Motion {
    pub fn heading(&self) -> Heading {
        match (self.displacement.0.abs(), self.displacement.1.abs()) {
            (0, 0) => Heading::Still,
            (0, _) | (_, 0) => Heading::Orthogonal,
            (dx, dy) if dx == dy => Heading::Diagonal,
            _ => Heading::Oblique,
        }
    }

    /// Cells per generation along the faster axis, as a reduced fraction.
    pub fn speed(&self) -> (u32, u32) {
        let distance = self.displacement.0.abs().max(self.displacement.1.abs()) as u32;
        let divisor = gcd(distance, self.period);
        (distance / divisor, self.period / divisor)
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The eight ways there are to go on the grid, clockwise from straight up; `y` points down.
pub const WAYS: [(i32, i32); 8] = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];

/// Which of the [`WAYS`] a displacement goes, as near as they tell: how many eighths of a turn
/// it is, clockwise from straight up. None for no displacement at all.
pub fn way(dx: i32, dy: i32) -> Option<usize> {
    WAYS.iter().position(|&way| way == (dx.signum(), dy.signum()))
}

/// A rotation or mirror of the plane that maps the block grid onto itself, and what it does
/// to a single block.
#[derive(Clone, Copy)]
struct Orientation {
    cell: fn(Cell) -> Cell,
    block: fn(u8) -> u8,
}

/// All of them: they turn and flip about the centre of the block at the origin.
const ORIENTATIONS: [Orientation; 8] = [
    Orientation { cell: |cell| cell, block: |block| block },
    Orientation { cell: |(x, y)| (1 - y, x), block: rotate_cw },
    Orientation { cell: |(x, y)| (1 - x, 1 - y), block: rotate_180 },
    Orientation { cell: |(x, y)| (y, 1 - x), block: rotate_ccw },
    Orientation { cell: |(x, y)| (1 - x, y), block: mirror },
    Orientation { cell: |(x, y)| (x, 1 - y), block: flip },
    Orientation { cell: |(x, y)| (y, x), block: transpose },
    Orientation { cell: |(x, y)| (1 - y, 1 - x), block: anti_transpose },
];

/// A study on its way, as whoever waits for it on another thread sees it: how far it has got,
/// and a way to say that this is far enough.
#[derive(Debug, Default)]
pub struct Watch {
    generation: AtomicU32,
    apart: AtomicBool,
    stopped: AtomicBool,
}

impl Watch {
    /// For how many generations the pattern has been followed so far.
    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Whether the pattern has been followed as far as it will be, and what it became is now
    /// being taken apart into its pieces.
    pub fn taking_apart(&self) -> bool {
        self.apart.load(Ordering::Relaxed)
    }

    /// Far enough: the pattern is followed no further, and what is known by now is the study.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

/// A block as a pattern comes by it: at an odd generation or at an even one, when the blocks
/// lie a cell further along both ways; and which of those blocks it is, row first.
type Block = (bool, (i32, i32));

/// Some cells of a pattern followed on their own: back where they were after `period`
/// generations, having come by `blocks` on the way.
struct Unit {
    cells: Vec<Cell>,
    period: u32,
    blocks: HashSet<Block>,
}

/// What a pattern is known to be by its pieces ([`Analyser::apart`]): the patterns that never
/// meet, as [`Analyser::parts`] gives them; after how many generations all of them are back at
/// once; and after how many the slowest of them is.
struct Apart {
    parts: Vec<Vec<Cell>>,
    period: u128,
    slowest: u32,
}

/// Recognises patterns of one rule.
#[derive(Clone)]
pub struct Analyser {
    /// The world of the rule ([`BlockRule::world`]): the tables it goes through as it acts
    /// on what differs from its vacuum, one for each generation until they repeat.
    tables: Vec<BlockRule>,
    /// The orientations under which that world looks the same, as indices into
    /// [`ORIENTATIONS`]: a pattern turned or flipped by one of them is the same pattern,
    /// travelling another way. They are those of every one of the tables, which may be more
    /// than the rule's own: a vacuum can have less symmetry than what happens on it.
    orientations: Vec<usize>,
    /// Where to stop: a pattern that has not repeated after this many generations, or has
    /// grown beyond this many cells or this extent, is not recognised.
    pub max_generations: u32,
    pub max_cells: usize,
    pub max_extent: i32,
    /// Where a study says how far it has got, and is told to stop, if anyone is watching.
    pub watch: Option<Arc<Watch>>,
}

impl Analyser {
    pub fn new(rule: &BlockRule) -> Self {
        let tables = rule.world();
        let looks_the_same = |&i: &usize| tables.iter().all(|table| table.commutes_with(ORIENTATIONS[i].block));
        Self {
            orientations: (0..ORIENTATIONS.len()).filter(looks_the_same).collect(),
            tables,
            max_generations: 8192,
            max_cells: 256,
            max_extent: 256,
            watch: None,
        }
    }

    /// Has whoever is watching said to stop?
    fn stopped(&self) -> bool {
        self.watch.as_ref().is_some_and(|watch| watch.stopped())
    }

    /// Runs the pattern alone until it repeats. `phase` says where the vacuum was in its cycle
    /// when the pattern was found.
    ///
    /// The canonical form is chosen among the forms the pattern takes through its period, each
    /// time the tables start over, and every orientation the rule allows, by a fixed order:
    /// the one travelling furthest right, then furthest down, then with the smallest bounding
    /// box, then first in reading order of its cells.
    pub fn analyse(&self, cells: &[Cell], phase: usize) -> Option<Motion> {
        self.analyse_as_found(cells, phase).map(|(motion, _)| motion)
    }

    /// As [`Analyser::analyse`], and besides how far the pattern moves in a period as it
    /// lies: the form it is filed under travels right or down, whichever way the pattern
    /// was going when it was found.
    pub fn analyse_as_found(&self, cells: &[Cell], phase: usize) -> Option<(Motion, (i32, i32))> {
        // Nothing at all is no pattern.
        if cells.is_empty() {
            return None;
        }
        self.motion_or_fate(cells, phase).ok()
    }

    /// As [`Analyser::analyse_as_found`], for a pattern of at least one cell; and of one that
    /// has no form to be filed under, what became of it instead: whether it got out of hand,
    /// or was only not followed for long enough.
    pub fn motion_or_fate(&self, cells: &[Cell], phase: usize) -> Result<(Motion, (i32, i32)), Fate> {
        // What does not come back is found out first, since most of what is asked about
        // does not.
        let returns = self.fate(cells, phase);
        let Fate::Returns { period, displacement: moved } = returns else {
            return Err(returns);
        };
        // Then through its period once more. How far the pattern moves is the same at every
        // phase, so of each orientation the form to keep is known as the forms come: a long
        // period takes no more room than a short one.
        let mut least: Vec<Option<Vec<Cell>>> = vec![None; self.orientations.len()];
        let keep = |form: &[Cell]| {
            for (least, &i) in least.iter_mut().zip(&self.orientations) {
                let (turned, _) = reorient(form, moved, &ORIENTATIONS[i]);
                if least.as_ref().is_none_or(|least| in_order(&turned, least).is_lt()) {
                    *least = Some(turned);
                }
            }
        };
        // Only someone who says to stop keeps the second time round from ending as the first.
        if self.run(cells, phase, keep, |_, _, _| {}) != returns {
            return Err(Fate::Undecided);
        }
        let seen = least.into_iter().zip(&self.orientations);
        let seen = seen.filter_map(|(form, &i)| Some((form?, reorient(&[], moved, &ORIENTATIONS[i]).1)));
        let least = seen
            .min_by(|(a, a_moved), (b, b_moved)| Reverse(a_moved).cmp(&Reverse(b_moved)).then_with(|| in_order(a, b)));
        let (canonical, displacement) = least.ok_or(Fate::Undecided)?;
        Ok((Motion { period, displacement, canonical }, moved))
    }

    /// What becomes of the pattern, without working out its canonical form: the quick way to
    /// tell what stays, what travels and what gets out of hand.
    pub fn fate(&self, cells: &[Cell], phase: usize) -> Fate {
        self.run(cells, phase, |_| {}, |_, _, _| {})
    }

    /// Everything the analyser can say about the pattern. None for no cells at all.
    pub fn study(&self, cells: &[Cell], phase: usize) -> Option<Study> {
        if cells.is_empty() {
            return None;
        }
        let start = self.start(cells, phase);
        // What is known by its pieces is followed for as long as the slowest of them takes to
        // be back: by then all of it has been seen, if not all of it at once.
        let apart = self.apart(&start);
        let patience = apart.as_ref().map_or(self.max_generations, |apart| apart.slowest);
        let follower = Analyser { max_generations: patience, ..self.clone() };
        let cycle = self.tables.len() as u32;
        let (mut fewest, mut most) = (start.len(), start.len());
        let (mut widest, mut highest) = extent(&start);
        let mut still = true;
        let mut generations = 0;
        // The cells where they lie, a generation ago and throughout; how many changed in all;
        // how many there were at every generation; and the last form.
        let mut before = start.clone();
        let mut throughout = start.clone();
        let mut changed = 0;
        let mut populations = Vec::new();
        let mut last = start.clone();
        let (mut recurs, mut cycles) = (None, 0);
        let fate = follower.run(
            cells,
            phase,
            |form| {
                // Every time the tables start over the form is on the same footing as the
                // start: the first time it is one of the start's turns or mirrors is noted.
                if cycles > 0 && recurs.is_none() {
                    recurs = self.turn_into(&start, form).map(|turn| (cycles * cycle, turn));
                }
                cycles += 1;
            },
            |_, pattern, moved| {
                generations += 1;
                fewest = fewest.min(pattern.len());
                most = most.max(pattern.len());
                let (width, height) = extent(pattern);
                widest = widest.max(width);
                highest = highest.max(height);
                // The frame moves with the blocks; the cells themselves need not have.
                let in_place: Vec<Cell> = pattern.iter().map(|&(x, y)| (x + moved.0, y + moved.1)).collect();
                still &= in_place == start;
                changed += differing(&before, &in_place);
                throughout = common(&throughout, &in_place);
                before = in_place;
                populations.push(pattern.len());
                last.clear();
                last.extend_from_slice(pattern);
            },
        );
        // The pattern came back: it is gone through once more for the form to file it under,
        // which is not for anyone to watch or to stop.
        let unwatched = Analyser { watch: None, ..self.clone() };
        let motion = match fate {
            Fate::Returns { .. } => unwatched.analyse(cells, phase),
            _ => None,
        };
        let period = match (fate, &apart) {
            (Fate::Returns { period, .. }, _) => Some(period as u128),
            (_, Some(apart)) => Some(apart.period),
            _ => None,
        };
        let parts = match (fate, apart) {
            (_, Some(apart)) => apart.parts,
            (Fate::Returns { period, .. }, None) => self.parts(&start, 0, period),
            _ => Vec::new(),
        };
        let growth = match period {
            Some(_) => None,
            None => Some(growth_of(&populations)),
        };
        let (pieces, more_pieces) = match period {
            // Parts that never meet come back each on its own, when the whole does at the latest.
            Some(_) if parts.len() > 1 => (parts.iter().map(|part| unwatched.piece(part).0).collect(), 0),
            Some(_) => (Vec::new(), 0),
            // What spreads over the plane is one thing, not pieces.
            None if growth.is_some_and(|growth| growth >= SPREADING) => (Vec::new(), 0),
            None => {
                if let Some(watch) = &self.watch {
                    watch.apart.store(true, Ordering::Relaxed);
                }
                self.pieces(&last)
            }
        };
        let symmetries = symmetries_of(&start);
        Some(Study {
            fate,
            period,
            motion,
            still,
            cells: (fewest, most),
            extent: (widest, highest),
            parts: parts.len(),
            generations,
            symmetry: symmetry_of(&symmetries),
            symmetries,
            recurs,
            heat: changed as f32 / generations.max(1) as f32,
            stator: throughout.len(),
            growth,
            pieces,
            more_pieces,
            start,
        })
    }

    /// The pattern settled and taken to the start of the tables' cycle, as [`Study::start`]
    /// has it.
    fn start(&self, cells: &[Cell], phase: usize) -> Vec<Cell> {
        let mut pattern = cells.to_vec();
        settle(&mut pattern);
        let mut phase = phase % self.tables.len();
        while phase != 0 {
            advance(&mut pattern, self.tables[phase].table());
            phase = (phase + 1) % self.tables.len();
        }
        pattern
    }

    /// The turn or mirror, of those the rule allows, that takes `form` into `start`, if one
    /// does.
    fn turn_into(&self, start: &[Cell], form: &[Cell]) -> Option<Turn> {
        self.orientations
            .iter()
            .filter(|&&i| i != 0)
            .find(|&&i| reorient(form, (0, 0), &ORIENTATIONS[i]).0 == start)
            .map(|&i| turn(i))
    }

    /// What became of a pattern that did not come back to its shape: its last form taken
    /// apart into the pieces that go their own ways, the largest first, each followed on its
    /// own for a while; and how many pieces beyond those were left alone.
    ///
    /// Which cells belong together is not a matter of distance: the ships of a stream fly
    /// closer to each other than the two halves of some ships lie. So the form is followed
    /// for a while longer with every cell's descent kept track of, as [`Analyser::parts`]
    /// does, and cells that have not come to share a block by then go their own ways.
    fn pieces(&self, cells: &[Cell]) -> (Vec<Piece>, usize) {
        let mut groups: Vec<usize> = (0..cells.len()).collect();
        let mut pattern: Vec<(Cell, usize)> = cells.iter().copied().zip(0..).collect();
        // Whole cycles of the tables, so that the pieces are at the start of one.
        let window = PIECE_WINDOW.next_multiple_of(self.tables.len() as u32) as usize;
        for table in self.tables.iter().cycle().take(window) {
            // Told to stop, there is no telling the pieces apart: none are given.
            if self.stopped() {
                return (Vec::new(), 0);
            }
            advance_groups(&mut pattern, table.table(), &mut groups);
        }
        let mut all: Vec<(usize, Vec<Cell>)> = Vec::new();
        for &(cell, group) in &pattern {
            let group = root(&mut groups, group);
            match all.iter_mut().find(|piece| piece.0 == group) {
                Some(piece) => piece.1.push(cell),
                None => all.push((group, vec![cell])),
            }
        }
        all.sort_by_key(|(_, piece)| Reverse(piece.len()));
        let mut work = PIECE_WORK;
        let mut pieces = Vec::new();
        for (_, piece) in &all {
            if work == 0 || self.stopped() {
                break;
            }
            if piece.len() > PIECE_CELLS {
                pieces.push(Piece { cells: piece.len(), kind: PieceKind::Unexamined, form: Vec::new() });
                continue;
            }
            let (piece, generations) = self.for_piece(piece, PIECE_GENERATIONS).piece(piece);
            work = work.saturating_sub(piece.cells as u64 * generations as u64);
            pieces.push(piece);
        }
        let more = all.len() - pieces.len();
        (pieces, more)
    }

    /// An analyser for a piece of something larger: it follows for so many generations, to
    /// four times the piece's cells and some way beyond its width, and nobody watches it.
    fn for_piece(&self, cells: &[Cell], generations: u32) -> Analyser {
        let (width, height) = extent(cells);
        Analyser {
            max_generations: generations,
            max_cells: (4 * cells.len()).max(64),
            max_extent: PIECE_EXTENT.max(width.max(height) + PIECE_EXTENT / 2),
            watch: None,
            ..self.clone()
        }
    }

    /// The pattern as several that never meet, if each of them is back where it was after a
    /// period of its own. Such a pattern is back when all of them are at once, after the
    /// least common multiple of their periods: what a blob leaves behind is a field of small
    /// oscillators that is back after billions of generations. There is no following it
    /// there, and it is known by its pieces instead.
    ///
    /// That pieces never meet is not seen by following them either. Each is followed on its
    /// own through its period, and the blocks it comes by are noted. Pieces that come by no
    /// block in common cannot meet, nor can one be found where the other was: all of them
    /// are back when each is, and no sooner. Pieces that do come by the same block are
    /// followed as one, which settles what they do to each other.
    ///
    /// None for a pattern of one piece; for one with a piece that is not back where it was
    /// within the work allowed ([`PIECE_WORK`]), because it travels, or grows, or takes
    /// long; and for one that is back later than there is counting.
    fn apart(&self, start: &[Cell]) -> Option<Apart> {
        let window = (PIECE_WINDOW as usize).next_multiple_of(self.tables.len());
        let mut work = PIECE_WORK.checked_sub((start.len() * window) as u64)?;
        // Cells that meet soon are one piece from the outset.
        let mut groups: Vec<usize> = (0..start.len()).collect();
        let mut pattern: Vec<(Cell, usize)> = start.iter().copied().zip(0..).collect();
        let mut count = start.len();
        for table in self.tables.iter().cycle().take(window) {
            if count < 2 || self.stopped() {
                return None;
            }
            count -= advance_groups(&mut pattern, table.table(), &mut groups);
        }
        if count < 2 {
            return None;
        }
        let mut fresh = grouped(start, &mut groups);
        let mut units: Vec<Unit> = Vec::new();
        while !fresh.is_empty() {
            for cells in fresh.drain(..) {
                if self.stopped() {
                    return None;
                }
                units.push(self.in_place(cells, &mut work)?);
            }
            // The units that come by a block in common, by way of others too, become one.
            let mut first: HashMap<Block, usize> = HashMap::new();
            let mut joined: Vec<usize> = (0..units.len()).collect();
            for (index, unit) in units.iter().enumerate() {
                for &block in &unit.blocks {
                    let other = root(&mut joined, *first.entry(block).or_insert(index));
                    let own = root(&mut joined, index);
                    joined[own] = other;
                }
            }
            let mut sizes = vec![0; units.len()];
            for index in 0..units.len() {
                sizes[root(&mut joined, index)] += 1;
            }
            let mut together: Vec<Vec<Cell>> = vec![Vec::new(); units.len()];
            let mut alone = Vec::new();
            for (index, unit) in units.into_iter().enumerate() {
                match root(&mut joined, index) {
                    group if sizes[group] > 1 => together[group].extend(unit.cells),
                    _ => alone.push(unit),
                }
            }
            units = alone;
            fresh = together.into_iter().filter(|cells| !cells.is_empty()).collect();
            for cells in &mut fresh {
                cells.sort_unstable_by_key(|&(x, y)| (y, x));
            }
        }
        if units.len() < 2 {
            return None;
        }
        let (mut period, mut slowest) = (1u128, 0);
        let mut parts = Vec::new();
        for unit in &units {
            let common = gcd(unit.period, (period % unit.period as u128) as u32);
            period = period.checked_mul((unit.period / common) as u128)?;
            slowest = slowest.max(unit.period);
            // Units that were followed as one may never have met, for all that.
            parts.extend(self.parts(&unit.cells, 0, unit.period));
        }
        parts.sort_unstable_by_key(|part| (part[0].1, part[0].0));
        Some(Apart { parts, period, slowest })
    }

    /// Follows some cells of a pattern on their own until they are back where they were, for
    /// no more than the work that is left, which is counted in cells times generations. None
    /// if they are not back by then, or back somewhere else.
    fn in_place(&self, cells: Vec<Cell>, work: &mut u64) -> Option<Unit> {
        let generations = (*work / cells.len() as u64).min(self.max_generations as u64) as u32;
        // The cells are followed from next to the origin, an even way from where they are.
        let mut settled = cells.clone();
        let origin = settle(&mut settled);
        let mut blocks = HashSet::new();
        let mut generation = 0;
        let fate = self.for_piece(&cells, generations).run(
            &settled,
            0,
            |_| {},
            |_, pattern, moved| {
                generation += 1;
                let odd = generation & 1;
                let (dx, dy) = (moved.0 + origin.0 - odd, moved.1 + origin.1 - odd);
                blocks.extend(pattern.iter().map(|&(x, y)| (odd == 1, place(&(x + dx, y + dy)).0)));
            },
        );
        *work = work.saturating_sub(cells.len() as u64 * generation as u64);
        match fate {
            Fate::Returns { period, displacement: (0, 0) } => Some(Unit { cells, period, blocks }),
            _ => None,
        }
    }

    /// What some cells do on their own, as a piece of something larger, and for how many
    /// generations they were followed to find out.
    fn piece(&self, cells: &[Cell]) -> (Piece, u32) {
        let (fate, still, growth, generations) = self.follow(cells, 0);
        let kind = match (fate, still) {
            (Fate::Returns { displacement: (0, 0), .. }, true) => PieceKind::StillLife,
            (Fate::Returns { period, displacement: (0, 0) }, false) => PieceKind::Oscillator { period },
            (Fate::Returns { period, displacement }, _) => PieceKind::Spaceship { period, displacement },
            // A gun among the pieces is too wide before it is too many cells too.
            (Fate::Grows | Fate::Scatters, _) if growth >= GROWING => PieceKind::Grows,
            (Fate::Grows, _) => PieceKind::Grows,
            (Fate::Scatters, _) => PieceKind::Scatters,
            (Fate::Undecided, _) => PieceKind::Undecided,
        };
        // What comes back is filed as the catcher files a ship: under one form, whichever way
        // it lies.
        let form = match fate {
            Fate::Returns { .. } => self.analyse(cells, 0).map_or(Vec::new(), |motion| motion.canonical),
            _ => Vec::new(),
        };
        (Piece { cells: cells.len(), kind, form }, generations)
    }

    /// The fate of a pattern; whether it kept still on the way; the power of time its cells
    /// went with; and for how many generations it was followed.
    fn follow(&self, cells: &[Cell], phase: usize) -> (Fate, bool, f32, u32) {
        let mut still = true;
        let mut populations = Vec::new();
        let fate = self.run(
            cells,
            phase,
            |_| {},
            |from, pattern, moved| {
                let in_place = pattern.iter().map(|&(x, y)| (x + moved.0, y + moved.1));
                still &= in_place.eq(from.iter().copied());
                populations.push(pattern.len());
            },
        );
        (fate, still, growth_of(&populations), populations.len() as u32)
    }

    /// A pattern filed at the start of the tables' cycle, as it is at every generation of the
    /// cycle: the forms to put on a grid whose vacuum is that far into its own cycle, each
    /// relative to a corner of the blocks the next step rewrites. For a rule whose vacuum
    /// stands still there is only the one.
    pub fn forms(&self, cells: &[Cell]) -> Vec<Vec<Cell>> {
        let mut pattern = settled(cells);
        let mut forms = Vec::with_capacity(self.tables.len());
        for table in &self.tables {
            forms.push(pattern.clone());
            advance(&mut pattern, table.table());
        }
        forms
    }

    /// Runs the pattern until it is back in its starting shape, as it lies, or until it is
    /// given up. Along the way `seen` gets its form every time the tables start over, the
    /// first time included, and `each` gets the form it set out with, the form after every
    /// generation and how far that has moved from where it started.
    fn run(
        &self,
        cells: &[Cell],
        phase: usize,
        mut seen: impl FnMut(&[Cell]),
        mut each: impl FnMut(&[Cell], &[Cell], (i32, i32)),
    ) -> Fate {
        // Forms are only comparable where the same table comes next: go to the first one.
        let mut pattern = self.start(cells, phase);
        let start = pattern.clone();
        let mut moved = (0, 0);
        let mut generation = 0;
        loop {
            seen(&pattern);
            for table in &self.tables {
                let (dx, dy) = advance(&mut pattern, table.table());
                moved = (moved.0 + dx, moved.1 + dy);
                each(&start, &pattern, moved);
            }
            generation += self.tables.len() as u32;
            let (width, height) = extent(&pattern);
            if pattern == start {
                return Fate::Returns { period: generation, displacement: moved };
            } else if pattern.len() > self.max_cells {
                return Fate::Grows;
            } else if width.max(height) > self.max_extent {
                return Fate::Scatters;
            } else if generation >= self.max_generations {
                return Fate::Undecided;
            }
            if let Some(watch) = &self.watch {
                watch.generation.store(generation, Ordering::Relaxed);
                if watch.stopped() {
                    return Fate::Undecided;
                }
            }
        }
    }

    /// The cells of a pattern that go with its first cell: those that come to share a block
    /// with it, or with cells that did, within `generations`. The ships of a dense stream lie
    /// within reach of each other without ever meeting; this tells the one at the edge of the
    /// grid from the ones behind it.
    pub fn with_first(&self, cells: &[Cell], phase: usize, generations: u32) -> Vec<Cell> {
        let mut groups: Vec<usize> = (0..cells.len()).collect();
        let mut pattern: Vec<(Cell, usize)> = cells.iter().copied().zip(0..).collect();
        for table in self.tables.iter().cycle().skip(phase).take(generations as usize) {
            advance_groups(&mut pattern, table.table(), &mut groups);
        }
        let first = root(&mut groups, 0);
        (0..cells.len()).filter(|&index| root(&mut groups, index) == first).map(|index| cells[index]).collect()
    }

    /// The pattern taken apart into the groups of cells that never meet, where cells meet by
    /// sharing a block. Every group is a pattern of its own: what repeats as a whole may be
    /// several spaceships flying side by side. `period` is that of the whole.
    pub fn parts(&self, cells: &[Cell], phase: usize, period: u32) -> Vec<Vec<Cell>> {
        // Every cell belongs to the group of the cells it descends from: at first its own.
        let mut groups: Vec<usize> = (0..cells.len()).collect();
        let mut pattern: Vec<(Cell, usize)> = cells.iter().copied().zip(0..).collect();
        let mut tables = self.tables.iter().cycle().skip(phase);
        let mut count = cells.len();
        // A period in which no groups met leaves them as they are for good.
        let mut quiet = false;
        while count > 1 && !quiet {
            let before = count;
            for table in tables.by_ref().take(period as usize) {
                count -= advance_groups(&mut pattern, table.table(), &mut groups);
                // All of a piece: there is nothing more to find out.
                if count == 1 {
                    break;
                }
            }
            quiet = count == before;
        }
        grouped(cells, &mut groups)
    }
}

/// The cells of a pattern group by group, where each cell's group is that of its index: the
/// groups in the order their first cells come in, and the cells of each in theirs.
fn grouped(cells: &[Cell], groups: &mut [usize]) -> Vec<Vec<Cell>> {
    let mut parts: Vec<Vec<Cell>> = Vec::new();
    let mut part_of = vec![usize::MAX; cells.len()];
    for (index, &cell) in cells.iter().enumerate() {
        let group = root(groups, index);
        if part_of[group] == usize::MAX {
            part_of[group] = parts.len();
            parts.push(Vec::new());
        }
        parts[part_of[group]].push(cell);
    }
    parts
}

/// The pattern next to the origin, cells in reading order: equal shapes on equal footing with
/// the block grid give equal lists.
pub fn settled(cells: &[Cell]) -> Vec<Cell> {
    let mut cells = cells.to_vec();
    settle(&mut cells);
    cells
}

/// Moves the pattern next to the origin without changing how it sits on the block grid, that
/// is by an even amount each way, and sorts its cells in reading order. Returns where it was.
fn settle(cells: &mut [Cell]) -> (i32, i32) {
    let even_floor = |values: &mut dyn Iterator<Item = i32>| values.min().unwrap_or(0) & !1;
    let x0 = even_floor(&mut cells.iter().map(|cell| cell.0));
    let y0 = even_floor(&mut cells.iter().map(|cell| cell.1));
    for cell in cells.iter_mut() {
        *cell = (cell.0 - x0, cell.1 - y0);
    }
    cells.sort_unstable_by_key(|&(x, y)| (y, x));
    (x0, y0)
}

/// The block a cell is in, row first, and the cell's bit in the state of that block.
fn place(&(x, y): &Cell) -> ((i32, i32), u8) {
    ((y >> 1, x >> 1), 1 << ((x & 1) + 2 * (y & 1)))
}

/// The cells of a block in `state`, as the step after sees them: its blocks are the shifted
/// ones, and coordinates are relative to a corner of those.
fn cells_of(block: (i32, i32), state: u8) -> impl Iterator<Item = Cell> {
    let (y, x) = block;
    (0..4).filter(move |bit| (state >> bit) & 1 == 1).map(move |bit| (2 * x + (bit & 1) - 1, 2 * y + (bit >> 1) - 1))
}

/// One generation: every block with a cell in it is rewritten. The pattern is settled again;
/// returns how far that and the shift of the blocks moved it.
fn advance(cells: &mut Vec<Cell>, table: &[u8; 16]) -> (i32, i32) {
    let mut bits: Vec<((i32, i32), u8)> = cells.iter().map(place).collect();
    bits.sort_unstable_by_key(|&(block, _)| block);
    cells.clear();
    for members in bits.chunk_by(|a, b| a.0 == b.0) {
        let state = members.iter().fold(0, |state, &(_, bit)| state | bit);
        cells.extend(cells_of(members[0].0, table[state as usize]));
    }
    let (x0, y0) = settle(cells);
    (x0 + 1, y0 + 1)
}

/// One generation of cells that each belong to a group. The cells of a block have met: their
/// groups become one, and what the block turns into belongs to it. Returns how many groups
/// fewer that leaves.
fn advance_groups(cells: &mut Vec<(Cell, usize)>, table: &[u8; 16], groups: &mut [usize]) -> usize {
    cells.sort_unstable_by_key(|(cell, _)| place(cell).0);
    let mut after = Vec::with_capacity(cells.len());
    let mut joined = 0;
    for members in cells.chunk_by(|a, b| place(&a.0).0 == place(&b.0).0) {
        let (block, _) = place(&members[0].0);
        let group = root(groups, members[0].1);
        let mut state = 0;
        for &(cell, other) in members {
            state |= place(&cell).1;
            let other = root(groups, other);
            if other != group {
                groups[other] = group;
                joined += 1;
            }
        }
        after.extend(cells_of(block, table[state as usize]).map(|cell| (cell, group)));
    }
    *cells = after;
    joined
}

/// The group that `group` has become part of, each group pointing at one that took it in.
fn root(groups: &mut [usize], mut group: usize) -> usize {
    while groups[group] != group {
        groups[group] = groups[groups[group]];
        group = groups[group];
    }
    group
}

/// The pattern and its movement as seen through an orientation.
fn reorient(cells: &[Cell], moved: (i32, i32), orientation: &Orientation) -> (Vec<Cell>, (i32, i32)) {
    let mut turned: Vec<Cell> = cells.iter().map(|&cell| (orientation.cell)(cell)).collect();
    settle(&mut turned);
    let (origin, tip) = ((orientation.cell)((0, 0)), (orientation.cell)(moved));
    (turned, (tip.0 - origin.0, tip.1 - origin.1))
}

/// The power of time that a pattern's cells went with, from how many it had at each
/// generation: from what it gained half way and in the end, so that a gun with a long stream
/// behind it still counts as growing along lines. 0 for a pattern that did not grow by a
/// quarter at least: what it gained is then no more than a flicker.
fn growth_of(populations: &[usize]) -> f32 {
    let count = populations.len();
    if count < 2 {
        return 0.0;
    }
    let (first, half_way, end) = (populations[0], populations[count / 2 - 1], populations[count - 1]);
    if (end as f32) < 1.25 * first as f32 {
        return 0.0;
    }
    let gained = |cells: usize| cells.saturating_sub(first).max(1) as f32;
    (gained(end) / gained(half_way)).log2().clamp(0.0, 3.0)
}

/// The turn or mirror that an orientation is.
fn turn(orientation: usize) -> Turn {
    match orientation {
        1 | 3 => Turn::Quarter,
        2 => Turn::Half,
        4 | 5 => Turn::Mirror,
        _ => Turn::DiagonalMirror,
    }
}

/// Which turns and mirrors leave a settled pattern as it is, as it sits on the blocks: one
/// answer for each of the [`ORIENTATIONS`].
fn symmetries_of(cells: &[Cell]) -> [bool; 8] {
    std::array::from_fn(|i| reorient(cells, (0, 0), &ORIENTATIONS[i]).0 == cells)
}

/// What the turns and mirrors that leave a pattern as it is come to.
fn symmetry_of(itself: &[bool; 8]) -> Symmetry {
    let quarter = itself[1] && itself[3];
    let half = itself[2];
    let mirror = itself[4] || itself[5];
    let diagonal = itself[6] || itself[7];
    match (quarter, half, mirror, diagonal) {
        (true, _, true, true) => Symmetry::All,
        (true, ..) => Symmetry::QuarterTurn,
        (_, true, true, _) => Symmetry::TwoMirrors,
        (_, true, _, true) => Symmetry::TwoDiagonalMirrors,
        (_, true, ..) => Symmetry::HalfTurn,
        (_, _, true, _) => Symmetry::Mirror,
        (_, _, _, true) => Symmetry::DiagonalMirror,
        _ => Symmetry::None,
    }
}

/// How many cells two sorted patterns do not have in common.
fn differing(a: &[Cell], b: &[Cell]) -> u64 {
    let key = |&(x, y): &Cell| (y, x);
    let (mut i, mut j, mut apart) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match key(&a[i]).cmp(&key(&b[j])) {
            std::cmp::Ordering::Less => (i, apart) = (i + 1, apart + 1),
            std::cmp::Ordering::Greater => (j, apart) = (j + 1, apart + 1),
            std::cmp::Ordering::Equal => (i, j) = (i + 1, j + 1),
        }
    }
    (apart + (a.len() - i) + (b.len() - j)) as u64
}

/// The cells two sorted patterns have in common.
fn common(a: &[Cell], b: &[Cell]) -> Vec<Cell> {
    let key = |&(x, y): &Cell| (y, x);
    let (mut i, mut j, mut both) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        match key(&a[i]).cmp(&key(&b[j])) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                both.push(a[i]);
                (i, j) = (i + 1, j + 1);
            }
        }
    }
    both
}

/// Width and height of the bounding box.
fn extent(cells: &[Cell]) -> (i32, i32) {
    let span = |values: &mut dyn Iterator<Item = i32>| {
        let (min, max) = values.fold((i32::MAX, i32::MIN), |(min, max), v| (min.min(v), max.max(v)));
        if min > max { 0 } else { max - min + 1 }
    };
    (span(&mut cells.iter().map(|cell| cell.0)), span(&mut cells.iter().map(|cell| cell.1)))
}

fn area(cells: &[Cell]) -> i32 {
    let (width, height) = extent(cells);
    width * height
}

/// The order in which forms that move alike are preferred: the smaller bounding box first,
/// then the cells in reading order.
fn in_order(a: &[Cell], b: &[Cell]) -> std::cmp::Ordering {
    let reading = |&(x, y): &Cell| (y, x);
    area(a).cmp(&area(b)).then_with(|| a.iter().map(reading).cmp(b.iter().map(reading)))
}

/// The pattern in the run-length encoding used for Life patterns: `b` dead, `o` alive, `$` end
/// of row, a count before any of them. It is written from the origin, which keeps the
/// pattern's place on the block grid. Cells must not have negative coordinates.
pub fn to_rle(cells: &[Cell]) -> String {
    let mut sorted = cells.to_vec();
    sorted.sort_unstable_by_key(|&(x, y)| (y, x));
    let mut rle = String::new();
    let run = |count: i32, symbol: char, rle: &mut String| match count {
        0 => {}
        1 => rle.push(symbol),
        _ => rle.push_str(&format!("{count}{symbol}")),
    };
    let (mut row, mut column) = (0, 0);
    let mut cells = sorted.iter().peekable();
    while let Some(&(x, y)) = cells.next() {
        if y > row {
            run(y - row, '$', &mut rle);
            (row, column) = (y, 0);
        }
        run(x - column, 'b', &mut rle);
        let mut alive = 1;
        while cells.next_if(|&&next| next == (x + alive, y)).is_some() {
            alive += 1;
        }
        run(alive, 'o', &mut rle);
        column = x + alive;
    }
    rle
}

/// The cells of a run-length encoded pattern.
pub fn from_rle(rle: &str) -> Result<Vec<Cell>, String> {
    let (mut x, mut y, mut count) = (0, 0, 0);
    let mut cells = Vec::new();
    for symbol in rle.chars() {
        let run = if count == 0 { 1 } else { count };
        match symbol {
            '0'..='9' => {
                count = count * 10 + symbol.to_digit(10).unwrap() as i32;
                continue;
            }
            'b' => x += run,
            'o' => {
                cells.extend((x..x + run).map(|x| (x, y)));
                x += run;
            }
            '$' => (x, y) = (0, y + run),
            _ => return Err(format!("{symbol:?} does not belong in a run-length encoded pattern")),
        }
        count = 0;
    }
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rules::PRESETS,
        universe::{Rng, Universe},
    };

    fn rule(name: &str) -> BlockRule {
        name.parse().unwrap()
    }

    fn analyse(name: &str, rle: &str) -> Motion {
        Analyser::new(&rule(name)).analyse(&from_rle(rle).unwrap(), 0).unwrap()
    }

    #[test]
    fn the_known_patterns_of_single_rotation_are_recognised() {
        // Periods and displacements as js-revca's own tests have them.
        for (rle, period, displacement, heading) in [
            ("o", 4, (0, 0), Heading::Still),
            ("b2o$b2o", 2, (0, 0), Heading::Still),
            ("$2o2$2o", 12, (2, 0), Heading::Orthogonal),
            ("2bo$obo$o", 48, (2, 2), Heading::Diagonal),
            ("o$o2$o$o", 368, (2, 2), Heading::Diagonal),
            ("b2obobo$4bo$4bo$4bo$6bo", 242, (4, 0), Heading::Orthogonal),
        ] {
            let motion = analyse("single-rotation", rle);
            assert_eq!((motion.period, motion.displacement), (period, displacement), "{rle}");
            assert_eq!(motion.heading(), heading, "{rle}");
            let cells = from_rle(rle).unwrap();
            let parts = Analyser::new(&rule("single-rotation")).parts(&cells, 0, period);
            assert_eq!(parts, [cells], "{rle} is one pattern");
        }
        assert_eq!(analyse("single-rotation", "$2o2$2o").speed(), (1, 6));
        assert_eq!(analyse("single-rotation", "o$o2$o$o").speed(), (1, 184));
    }

    #[test]
    fn a_way_is_so_many_eighths_of_a_turn() {
        // Up is none, and round it goes with the clock: right a quarter, down a half.
        assert_eq!([way(0, -3), way(2, -2), way(5, 0), way(1, 1)], [Some(0), Some(1), Some(2), Some(3)]);
        assert_eq!([way(0, 4), way(-1, 1), way(-7, 0), way(-2, -2)], [Some(4), Some(5), Some(6), Some(7)]);
        assert_eq!(way(0, 0), None);
        // A pattern is filed flying right or down, and found flying whichever way it was.
        let analyser = Analyser::new(&rule("single-rotation"));
        let ship = from_rle("$2o2$2o").unwrap();
        for (orientation, flying) in [(0, (2, 0)), (1, (0, 2)), (2, (-2, 0)), (3, (0, -2))] {
            let (turned, _) = reorient(&ship, (0, 0), &ORIENTATIONS[orientation]);
            let (motion, moved) = analyser.analyse_as_found(&turned, 0).unwrap();
            assert_eq!((motion.displacement, moved), ((2, 0), flying), "turned {orientation} quarters");
        }
    }

    #[test]
    fn an_odd_period_moves_by_an_odd_amount() {
        // Conway's glider shape: the same shape on the other partition after 15 generations.
        let motion = analyse("single-rotation", "3o$o$bo");
        assert_eq!((motion.period, motion.displacement), (15, (1, 1)));
    }

    #[test]
    fn what_falls_apart_or_never_repeats_is_not_recognised() {
        let analyser = Analyser::new(&rule("single-rotation"));
        assert_eq!(analyser.analyse(&[], 0), None);
        // Two ships flying apart.
        let mut cells = from_rle("$2o2$2o").unwrap();
        cells.extend([(-20, 1), (-21, 1), (-20, 3), (-21, 3)]);
        assert_eq!(analyser.analyse(&cells, 0), None);
        assert_eq!(analyser.fate(&cells, 0), Fate::Scatters);
        // The same told by a fate: the lone cell stays, the ship travels.
        assert_eq!(analyser.fate(&[(0, 0)], 0), Fate::Returns { period: 4, displacement: (0, 0) });
        let ship = from_rle("$2o2$2o").unwrap();
        assert_eq!(analyser.fate(&ship, 0), Fate::Returns { period: 12, displacement: (2, 0) });
        // In ESPCA-0925bf a single cell grows into a disk; nothing is given the time it takes.
        let growing = Analyser::new(&BlockRule::from_espca("0925bf").unwrap());
        assert_eq!(growing.fate(&[(0, 0)], 0), Fate::Grows);
        let mut impatient = Analyser::new(&rule("single-rotation"));
        impatient.max_generations = 8;
        assert_eq!(impatient.fate(&ship, 0), Fate::Undecided);
    }

    #[test]
    fn patterns_that_never_meet_are_told_apart() {
        let analyser = Analyser::new(&rule("single-rotation"));
        let ship = from_rle("$2o2$2o").unwrap();
        // Two of the lightest ship abreast, and a third far behind: together they repeat like
        // one ship, yet each flies on its own.
        let abreast: Vec<Cell> = ship.iter().map(|&(x, y)| (x, y + 6)).collect();
        let behind: Vec<Cell> = ship.iter().map(|&(x, y)| (x - 40, y + 2)).collect();
        let fleet = [ship.clone(), abreast.clone(), behind.clone()].concat();
        let motion = analyser.analyse(&fleet, 0).unwrap();
        assert_eq!((motion.period, motion.displacement, motion.canonical.len()), (12, (2, 0), 12));
        assert_eq!(analyser.parts(&fleet, 0, motion.period), [ship, abreast, behind]);
        // Cells that orbit each other without ever sharing a block would be told apart as
        // well: a lone cell is one part, and nothing at all is none.
        assert_eq!(analyser.parts(&[(0, 0)], 0, 4), [[(0, 0)]]);
        assert!(analyser.parts(&[], 0, 4).is_empty());
    }

    /// The pattern after each of `generations` steps, in the coordinates it was given in.
    fn history(analyser: &Analyser, cells: &[Cell], generations: u32) -> Vec<Vec<Cell>> {
        let mut pattern = cells.to_vec();
        let mut origin = settle(&mut pattern);
        let mut tables = analyser.tables.iter().cycle();
        (0..generations)
            .map(|_| {
                let (dx, dy) = advance(&mut pattern, tables.next().unwrap().table());
                origin = (origin.0 + dx, origin.1 + dy);
                pattern.iter().map(|&(x, y)| (x + origin.0, y + origin.1)).collect()
            })
            .collect()
    }

    /// Parts do not need each other: run alone, they add up to what the whole does.
    #[test]
    fn parts_do_alone_what_they_do_together() {
        let mut rng = Rng::new(5);
        let mut rules: Vec<BlockRule> = PRESETS.iter().map(|preset| preset.rule()).collect();
        rules.extend((0..30).map(|_| BlockRule::random(|| rng.next_u64())));
        // Composites met, and those among them with a part of more than one cell.
        let (mut composite, mut grown) = (0, 0);
        for rule in rules {
            let mut analyser = Analyser::new(&rule);
            analyser.max_generations = 60;
            for _ in 0..60 {
                // Two clumps of cells, near enough to meet at times.
                let mut cells: Vec<Cell> = Vec::new();
                for x0 in [0, 4 + (rng.next_u64() % 8) as i32] {
                    for _ in 0..1 + rng.next_u64() % 3 {
                        cells.push((x0 + (rng.next_u64() % 3) as i32, (rng.next_u64() % 3) as i32));
                    }
                }
                cells.sort_unstable();
                cells.dedup();
                let Fate::Returns { period, .. } = analyser.fate(&cells, 0) else {
                    continue;
                };
                let parts = analyser.parts(&cells, 0, period);
                if parts.len() == 1 {
                    assert_eq!(parts[0], cells);
                    continue;
                }
                composite += 1;
                grown += (parts.len() < cells.len()) as usize;
                let generations = period * (cells.len() as u32 + 1);
                let whole = history(&analyser, &cells, generations);
                let alone: Vec<_> = parts.iter().map(|part| history(&analyser, part, generations)).collect();
                for (generation, whole) in whole.iter().enumerate() {
                    let mut together: Vec<Cell> = alone.iter().flat_map(|part| part[generation].clone()).collect();
                    together.sort_unstable_by_key(|&(x, y)| (y, x));
                    assert_eq!(*whole, together, "{rule}: {parts:?} after {generation}");
                }
            }
        }
        assert!(composite > 50 && grown > 50, "{composite} composites, {grown} with a grown part");
    }

    #[test]
    fn the_canonical_form_does_not_depend_on_phase_or_orientation() {
        let analyser = Analyser::new(&rule("single-rotation"));
        assert_eq!(analyser.orientations.len(), 4, "the rule has rotations but no mirrors");
        for rle in ["$2o2$2o", "2bo$obo$o", "3o$o$bo"] {
            let expected = analyse("single-rotation", rle);
            let mut pattern = from_rle(rle).unwrap();
            for _ in 0..expected.period.min(60) {
                for &orientation in &analyser.orientations {
                    let (turned, _) = reorient(&pattern, (0, 0), &ORIENTATIONS[orientation]);
                    assert_eq!(analyser.analyse(&turned, 0).as_ref(), Some(&expected), "{rle}");
                }
                advance(&mut pattern, analyser.tables[0].table());
            }
        }
        // It is one of the pattern's own phases, travelling right: of those with the smallest
        // bounding box, the one whose first cell comes first in reading order.
        let ship = analyse("single-rotation", "$2o2$2o");
        assert_eq!(to_rle(&ship.canonical), "b2o2$b2o");
    }

    #[test]
    fn a_vacuum_that_only_flickers_is_seen_through() {
        // ESPCA-fb3510 fills every empty block, but it is its own complement: on what differs
        // from its vacuum it acts as ESPCA-04caef does, at every generation. A ship is then
        // the same ship whichever generation it is found in.
        let flickering = BlockRule::from_espca("fb3510").unwrap();
        assert_eq!(flickering.vacuum_cycle(), [0, 15]);
        assert!(flickering.is_complement_symmetric());
        let analyser = Analyser::new(&flickering);
        assert_eq!(analyser.tables, [BlockRule::from_espca("04caef").unwrap()]);
        for rle in ["b2o2$b2o", "3o2$2bo"] {
            let ship = from_rle(rle).unwrap();
            let motion = analyser.analyse(&ship, 0).unwrap();
            assert_ne!(motion.heading(), Heading::Still, "{rle}");
            assert_eq!(analyser.analyse(&ship, 1), Some(motion), "{rle}");
        }
        // Critters is not its own complement: there the two generations differ.
        assert_eq!(Analyser::new(&rule("critters")).tables.len(), 2);
    }

    #[test]
    fn a_world_may_look_the_same_where_its_vacuum_does_not() {
        // Empty space under this rule goes through two diagonals, which no quarter turn leaves
        // alone: the rule looks the same after a half turn only. What differs from the vacuum
        // looks the same after every turn, and so a ship flying down and the same ship flying
        // right are one kind.
        let rule: BlockRule = "6,2,7,5,14,3,15,11,4,0,12,1,10,8,13,9".parse().unwrap();
        assert_eq!(rule.vacuum_cycle(), [0, 6, 15, 9]);
        assert!(rule.commutes_with(rotate_180) && !rule.commutes_with(rotate_cw));
        let analyser = Analyser::new(&rule);
        assert_eq!(analyser.orientations, [0, 1, 2, 3]);
        let ship = vec![(1, 0), (1, 2), (2, 2), (3, 1)];
        let (turned, _) = reorient(&ship, (0, 0), &ORIENTATIONS[1]);
        let (down, right) = (analyser.analyse(&ship, 0).unwrap(), analyser.analyse(&turned, 0).unwrap());
        assert_eq!((down.period, down.heading()), (68, Heading::Orthogonal));
        assert_eq!(down, right);
    }

    #[test]
    fn a_study_says_what_a_pattern_does() {
        let analyser = Analyser::new(&rule("single-rotation"));
        // The lightest spaceship: of a piece, four cells throughout, two dominoes that mirror
        // each other top to bottom. The rule has no mirrors, so it is never one of its own
        // orientations before its period is up, whatever its shape.
        let ship = analyser.study(&from_rle("b2o2$b2o").unwrap(), 0).unwrap();
        assert_eq!(ship.fate, Fate::Returns { period: 12, displacement: (2, 0) });
        assert_eq!(ship.motion.as_ref().map(|m| m.speed()), Some((1, 6)));
        assert!(!ship.still);
        assert_eq!((ship.cells, ship.parts, ship.generations), ((4, 4), 1, 12));
        assert_eq!(ship.start, from_rle("b2o2$b2o").unwrap());
        assert_eq!((ship.symmetry, ship.recurs), (Symmetry::Mirror, None));
        // On the blocks it is itself as it is and mirrored left to right, and not top to bottom,
        // where its rows would come to lie across the blocks the other way.
        assert_eq!(ship.symmetries, [true, false, false, false, true, false, false, false]);
        assert!(ship.heat > 0.0 && ship.stator == 0 && ship.growth.is_none() && ship.pieces.is_empty());
        // A lone cell goes round in four generations, a quarter turn at a time: two cells
        // change every generation, and none stays. In the corner of its block, it is itself
        // only across the diagonal through that corner.
        let cell = analyser.study(&[(0, 0)], 0).unwrap();
        assert_eq!(cell.fate, Fate::Returns { period: 4, displacement: (0, 0) });
        assert!(!cell.still && cell.extent == (1, 1));
        assert_eq!((cell.symmetry, cell.recurs), (Symmetry::DiagonalMirror, Some((1, Turn::Quarter))));
        assert_eq!(cell.symmetries, [true, false, false, false, false, false, true, false]);
        assert_eq!((cell.heat, cell.stator), (2.0, 0));
        // A block straddling the partitions never changes: a still life. (Cells are relative
        // to a corner of the blocks the next step rewrites, so this one lies across two of
        // them, and across two of the other partition's as well.) Turned a quarter it would
        // straddle them the other way, so it has the two mirrors but not the quarter turn.
        let block = analyser.study(&[(0, 1), (1, 1), (0, 2), (1, 2)], 0).unwrap();
        assert_eq!(block.fate, Fate::Returns { period: 2, displacement: (0, 0) });
        assert!(block.still);
        assert_eq!((block.symmetry, block.heat, block.stator), (Symmetry::TwoMirrors, 0.0, 4));
        assert_eq!(block.symmetries, [true, false, true, false, true, true, false, false]);
        // Two ships side by side are two patterns, and each is a piece: the same ship twice.
        let mut pair = from_rle("b2o2$b2o").unwrap();
        pair.extend(from_rle("b2o2$b2o").unwrap().iter().map(|&(x, y)| (x, y + 6)));
        let pair = analyser.study(&pair, 0).unwrap();
        assert_eq!((pair.parts, pair.pieces.len(), pair.more_pieces), (2, 2, 0));
        for piece in &pair.pieces {
            assert_eq!(piece.kind, PieceKind::Spaceship { period: 12, displacement: (2, 0) });
            assert_eq!((piece.cells, to_rle(&piece.form).as_str()), (4, "b2o2$b2o"));
        }
        // A pattern that grows is followed until it has too many cells. This one spreads
        // over the plane: its cells go with the square of time, and it is all one piece,
        // which is not taken apart.
        let mut small = Analyser::new(&rule("espca-0925bf"));
        small.max_cells = 50;
        let disk = small.study(&[(0, 0)], 0).unwrap();
        assert_eq!(disk.fate, Fate::Grows);
        assert!(disk.cells.1 > 50 && disk.extent.0 > 4 && disk.generations > 4, "{disk:?}");
        assert_eq!((disk.motion, disk.parts), (None, 0));
        assert!(disk.growth.is_some_and(|growth| growth >= SPREADING), "{:?}", disk.growth);
        assert!(disk.pieces.is_empty());
        assert_eq!(analyser.study(&[], 0), None);
    }

    #[test]
    fn a_study_can_be_watched_and_stopped() {
        // The slow diagonal ship of Single rotation takes 368 generations to come back.
        let mut analyser = Analyser::new(&rule("single-rotation"));
        let watch = Arc::new(Watch::default());
        analyser.watch = Some(watch.clone());
        let ship = from_rle("o$o2$o$o").unwrap();
        let study = analyser.study(&ship, 0).unwrap();
        assert_eq!(study.fate, Fate::Returns { period: 368, displacement: (2, 2) });
        assert_eq!(study.motion.as_ref().map(|motion| motion.period), Some(368));
        assert_eq!(watch.generation(), 367, "the last generation before it was back");
        assert!(!watch.taking_apart() && !watch.stopped());
        // Told to stop before it begins, the study is over after one generation, undecided,
        // and nothing is taken apart.
        watch.stop();
        let study = analyser.study(&ship, 0).unwrap();
        assert_eq!((study.fate, study.generations), (Fate::Undecided, 1));
        assert!(study.motion.is_none() && study.pieces.is_empty());
    }

    /// What a blob left behind under Single rotation, after two million generations inside
    /// an open border: all that could leave has left.
    const LEFT_BEHIND: &str = "\
        41bo2$31bo4$36bo6$59bo$44bo5$48bo8bo5$67bo2$59bo6bo2$87bo$62bobo$54bo22bo$72bo29bo2$85bo22bo$61bo$14bo$\
        35bo47b3o$63b2o4bo11bo25bo$36bo$19bo2$46bo$23bo28bo$69bo$27bo5bo3bo12bo$97bo$14bo40bo32bo8bo$52bo20bo$\
        18bo23bo44bo$5bo$102bo$21bo64bo14bo5bo$50bo$51b2o5bo23bo15bo$5bo17bo5bo20b3o33bo5bo$6bo21bobo18bobo$\
        49b2o11b2o$22bo39b2o50bo$11bo3bo3bo28bo31b2o18bo4bo$37bo50bo7bo8bo$24bo15b2o74bo$\
        5bo18bo15b2o16bo40bo9bo$62bo$12bo48bo15b2o$bo25bo31bo17b2o41bo$63bo13bo6bo$49bo6b2o5bo2b2o8bo7bo5b2o$\
        2bo8bo44b3o7b2o8bo13b2o$57b2o18bobo$6bo2bo22bo44b2o21bo$o11bo50bo15b3o35bo$\
        11bo7bobo27b2o10bo18b2o21bo21bo$6bo28bo8bo4b2o49bo$24bo73bo$o17bo12bo81bo$43b2o15bo15bo27bo$\
        40bo2b2o74bo$2bo8bo$36bo77bo$33bo67bo$34bobo4bo45bo$2bo55bo$7bo10bo28bo$28bo22b2o33bo20bo9bo$\
        20bo30b2o3bo2bo44bo11bo$7bo12bo12bo9bo27bo18bo$o21bo3bo4bo48b2o25b2o7bo$21bo58b2o$78b2o16bo3bo$\
        32bo28bo12bo3b2obo41bo$32b2o42bo18bo$32b2o31b4o20bo31bo$65bo2bo28bo14bo$65b3o17bo17bo$90bo$11bo65bo$\
        60b2o31bo$16bo30b2o11bobo13bo$9bo4bo11bo2bo31b2o10bo38bo$18bo52bo$65bo15bo$6bo64b2o$41bo29b2o7bo22bo$\
        99bo$11bo21bo50bo29bo2$53bo45bo28bo$18bo37bo18bo29bo$50bo40bo8bo8bo$54bo22bo13bo$48bo53bo$28bo3bo$\
        38bo26bo4bo36bo$94bo$32bo$25bo22bo34b2o12bo$83b2o25bo$79bo4bo13bo18bo2b2o2$21bo16bo2bo3bo17bo2bo18bo3bo$\
        57bo$19bo37bo26bo18bo9bo$47bo24bo$17bo43bo15bo26bo$22bo13bo37bo32bo2bo$28bo20bo8bo$42bo37b2o18bo11bo$\
        109bo$97bo$21bo73bo$28bo17bo26bo$37bo25bo39bo$56bo$78bo4bo$28bobo14b2o$61bo$60bo34bo$53bo2$107bo$\
        32bo65bo$92bo4bo$99bo2$66bo4bo2$29bo5bo$38bo9bo$45bo25bo$51bo$27bo$40bo$72bo2$23bo3bo";

    #[test]
    fn what_a_blob_leaves_behind_is_known_by_its_pieces() {
        let cells = from_rle(LEFT_BEHIND).unwrap();
        let mut analyser = Analyser::new(&rule("single-rotation"));
        (analyser.max_cells, analyser.max_extent) = (4 * cells.len(), 512);
        // Followed as it lies, it is not back after eight thousand generations, nor would it
        // be after a billion.
        assert_eq!(analyser.fate(&cells, 0), Fate::Undecided);
        // It is 271 patterns that never meet: ten that keep still, and 261 that go round in
        // periods of their own, from the lone cell's 4 to 590. All of them are back at once
        // after the least common multiple of those, 2⁵·3²·5·7·11·13·19·43·59 generations.
        // It was followed for as long as the slowest of them takes.
        let study = analyser.study(&cells, 0).unwrap();
        assert_eq!(study.period, Some(69_481_732_320));
        assert_eq!((study.fate, study.generations), (Fate::Undecided, 590));
        assert_eq!((study.parts, study.pieces.len(), study.more_pieces), (271, 271, 0));
        let mut periods: Vec<(u32, usize)> = Vec::new();
        let mut still = 0;
        for piece in &study.pieces {
            assert!(!piece.form.is_empty());
            match piece.kind {
                PieceKind::StillLife => still += 1,
                PieceKind::Oscillator { period } => match periods.iter_mut().find(|(known, _)| *known == period) {
                    Some((_, count)) => *count += 1,
                    None => periods.push((period, 1)),
                },
                kind => panic!("a piece that is {kind:?}"),
            }
        }
        periods.sort_unstable();
        let expected = [
            (4, 215),
            (8, 8),
            (16, 22),
            (24, 1),
            (28, 4),
            (36, 2),
            (40, 1),
            (52, 2),
            (76, 1),
            (104, 1),
            (176, 1),
            (288, 1),
            (344, 1),
            (590, 1),
        ];
        assert_eq!((periods.as_slice(), still), (&expected[..], 10));
        assert_eq!((study.cells, study.extent, study.stator), ((415, 415), (129, 155), 92));
        assert!(study.motion.is_none() && study.growth.is_none() && !study.still);
    }

    /// What its pieces say of a pattern is what following it finds, where it can be followed
    /// that far.
    #[test]
    fn the_pieces_say_what_following_finds() {
        let mut rng = Rng::new(11);
        let mut rules: Vec<BlockRule> = PRESETS.iter().map(|preset| preset.rule()).collect();
        rules.extend((0..30).map(|_| BlockRule::random(|| rng.next_u64())));
        // Patterns that came back, and those among them known by their pieces alone.
        let (mut back, mut by_pieces) = (0, 0);
        for rule in rules {
            let analyser = Analyser::new(&rule);
            for _ in 0..40 {
                // A few clumps of cells, some near enough to meet or to come by the same places.
                let mut cells: Vec<Cell> = Vec::new();
                for _ in 0..2 + rng.next_u64() % 4 {
                    let (x0, y0) = ((rng.next_u64() % 24) as i32, (rng.next_u64() % 24) as i32);
                    for _ in 0..1 + rng.next_u64() % 3 {
                        cells.push((x0 + (rng.next_u64() % 3) as i32, y0 + (rng.next_u64() % 3) as i32));
                    }
                }
                cells.sort_unstable();
                cells.dedup();
                let study = analyser.study(&cells, 0).unwrap();
                match analyser.fate(&cells, 0) {
                    Fate::Returns { period, .. } => {
                        assert_eq!(study.period, Some(period as u128), "{rule}: {cells:?}");
                        let parts = analyser.parts(&settled(&cells), 0, period);
                        assert_eq!(study.parts, parts.len(), "{rule}: {cells:?}");
                        back += 1;
                        by_pieces += (study.fate == Fate::Undecided) as usize;
                    }
                    // Back later than it is followed for, if at all.
                    Fate::Undecided => assert!(study.period.is_none_or(|period| period > 8192), "{rule}: {cells:?}"),
                    _ => assert_eq!(study.period, None, "{rule}: {cells:?}"),
                }
            }
        }
        assert!(back > 300 && by_pieces > 50, "{back} came back, {by_pieces} known by their pieces");
    }

    #[test]
    fn what_a_pattern_came_apart_into_is_told_piece_by_piece() {
        // Two of the lightest ships, one turned about, flying away from each other: too far
        // apart soon enough, and then each is a spaceship on its own.
        let mut analyser = Analyser::new(&rule("single-rotation"));
        analyser.max_extent = 40;
        let mut pair = from_rle("b2o2$b2o").unwrap();
        let (back, _) = reorient(&pair, (0, 0), &ORIENTATIONS[2]);
        pair.extend(back.iter().map(|&(x, y)| (x - 10, y)));
        let study = analyser.study(&pair, 0).unwrap();
        assert_eq!(study.fate, Fate::Scatters);
        let kinds: Vec<PieceKind> = study.pieces.iter().map(|piece| piece.kind).collect();
        assert!(
            kinds.contains(&PieceKind::Spaceship { period: 12, displacement: (2, 0) })
                && kinds.contains(&PieceKind::Spaceship { period: 12, displacement: (-2, 0) })
                && kinds.len() == 2,
            "{kinds:?}"
        );
        assert_eq!((study.more_pieces, study.symmetry), (0, Symmetry::HalfTurn));
        // Both are filed under the one form: the ship as it flies to the right.
        assert!(study.pieces.iter().all(|piece| to_rle(&piece.form) == "b2o2$b2o"), "{:?}", study.pieces);

        // A gun from a single cell: it grows along lines, and what it had sent out by the time
        // it was given up on is spaceships.
        let mut analyser = Analyser::new(&rule("four-way-gun"));
        analyser.max_cells = 200;
        let gun = analyser.study(&[(0, 0)], 0).unwrap();
        assert_eq!(gun.fate, Fate::Grows);
        assert!(gun.growth.is_some_and(|growth| growth < SPREADING), "{:?}", gun.growth);
        let ships = gun.pieces.iter().filter(|piece| matches!(piece.kind, PieceKind::Spaceship { .. })).count();
        assert!(ships >= 4 && ships + 1 >= gun.pieces.len(), "{:?}", gun.pieces);
    }

    #[test]
    fn a_pattern_has_a_form_for_every_generation_of_the_vacuum() {
        // Under Single rotation the vacuum stands still: the one form is the pattern itself.
        let ship = from_rle("b2o2$b2o").unwrap();
        assert_eq!(Analyser::new(&rule("single-rotation")).forms(&ship), [settled(&ship)]);

        // Critters' glider, found at generation 0, as a world at generation 1 has it: the
        // forms put on a grid at those phases fly on as the glider does.
        let critters = rule("critters");
        let analyser = Analyser::new(&critters);
        let glider = from_rle("$bo$2bo$2bo$bo").unwrap();
        let forms = analyser.forms(&glider);
        assert_eq!(forms.len(), 2);
        assert_eq!(forms[0], settled(&glider));
        assert_ne!(forms[1], forms[0]);
        for (phase, form) in forms.iter().enumerate() {
            let mut universe = Universe::new(32, 32, critters.clone());
            universe.step_by(phase as i64);
            let offset = universe.partition_offset() as i32;
            for &(x, y) in form {
                universe.set((8 + offset + x) as usize, (8 + offset + y) as usize, true);
            }
            let before = universe.population();
            let cells = |universe: &Universe| -> Vec<(usize, usize)> {
                (0..32).flat_map(|y| (0..32).map(move |x| (x, y))).filter(|&(x, y)| universe.get(x, y)).collect()
            };
            let start = cells(&universe);
            universe.step_by(4);
            assert_eq!(universe.population(), before, "phase {phase}: the glider came apart");
            let moved: Vec<(usize, usize)> = start.iter().map(|&(x, y)| (x + 2, y)).collect();
            assert_eq!(cells(&universe), moved, "phase {phase}: not the glider");
        }
    }

    #[test]
    fn orientations_do_to_cells_what_they_do_to_blocks() {
        for orientation in ORIENTATIONS {
            for state in 0..16u8 {
                let cells = (0..4).filter(|bit| (state >> bit) & 1 == 1).map(|bit| (bit & 1, bit >> 1));
                let turned =
                    cells.map(|cell| (orientation.cell)(cell)).fold(0, |turned, (x, y)| turned | 1 << (x + 2 * y));
                assert_eq!(turned, (orientation.block)(state));
            }
            // The block grid stays where it is: the four cells of a block stay one block.
            let corners = [(6, -4), (7, -4), (6, -3), (7, -3)].map(orientation.cell);
            assert!(corners.iter().all(|&(x, y)| (x >> 1, y >> 1) == (corners[0].0 >> 1, corners[0].1 >> 1)));
        }
    }

    /// What the analysis says must be what the grid does: for any rule, any small pattern.
    #[test]
    fn analysis_agrees_with_the_grid() {
        let mut rng = Rng::new(31);
        let mut rules: Vec<BlockRule> = PRESETS.iter().map(|preset| preset.rule()).collect();
        rules.extend((0..30).map(|_| BlockRule::random(|| rng.next_u64())));
        let (size, middle) = (96, 48);
        let mut recognised = 0;
        for rule in rules {
            let mut analyser = Analyser::new(&rule);
            analyser.max_generations = 40;
            for _ in 0..40 {
                let cells: Vec<Cell> = (0..3 + rng.next_u64() % 5)
                    .map(|_| ((rng.next_u64() % 5) as i32, (rng.next_u64() % 5) as i32))
                    .collect();
                let Fate::Returns { period, displacement: moved } = analyser.fate(&cells, 0) else {
                    continue;
                };
                recognised += 1;
                let mut universe = Universe::new(size, size, rule.clone());
                for &(x, y) in &cells {
                    universe.set((middle + x) as usize, (middle + y) as usize, true);
                }
                let start = universe.cells().to_vec();
                universe.step_by(period as i64);
                let shifted: Vec<u8> = (0..size * size)
                    .map(|i| {
                        let (x, y) = ((i % size) as i32 - moved.0, (i / size) as i32 - moved.1);
                        start[(y.rem_euclid(size as i32) * size as i32 + x.rem_euclid(size as i32)) as usize]
                    })
                    .collect();
                assert_eq!(universe.cells(), shifted, "{rule}: {cells:?} after {period}");
            }
        }
        assert!(recognised > 50, "only {recognised} patterns repeated");
    }

    #[test]
    fn run_length_encoding_round_trips() {
        for rle in ["o", "b2o$b2o", "$2o2$2o", "2bo$obo$o", "b2obobo$4bo$4bo$4bo$6bo", "3$10bo"] {
            assert_eq!(to_rle(&from_rle(rle).unwrap()), rle);
        }
        assert_eq!(from_rle("2o$bo").unwrap(), [(0, 0), (1, 0), (1, 1)]);
        assert_eq!(to_rle(&[]), "");
        assert!(from_rle("2o!x").is_err());
    }
}

/// Patterns from Morita's book, placed as its figures place them: a particle in the top, right,
/// bottom or left part of the cell in a column and a row of the figure, rows counted from the
/// top. Together with the rule numbers ([`BlockRule::from_espca`]) they pin down how his
/// automata lie on the block grid: which way is north, and which way the rules turn.
#[cfg(test)]
mod morita {
    use super::*;
    use crate::universe::Universe;

    const STILL: (i32, i32) = (0, 0);

    fn rule(number: &str) -> BlockRule {
        BlockRule::from_espca(number).unwrap()
    }

    /// Where the particles of a figure are on the block grid. A particle sits on the edge it
    /// is about to cross, and the edges are the cells here.
    fn sites(particles: &[(char, i32, i32)]) -> Vec<Cell> {
        let site = |&(part, column, row): &(char, i32, i32)| {
            // The book's y points up.
            let (x, y) = (column, -row);
            match part {
                'T' => (x + y, x - y - 1),
                'R' => (x + y, x - y),
                'B' => (x + y - 1, x - y),
                'L' => (x + y - 1, x - y - 1),
                _ => panic!("{part:?} is not a part of a cell"),
            }
        };
        particles.iter().map(site).collect()
    }

    /// A figure as a pattern: relative to a corner of the blocks the next step rewrites. Those
    /// are the cells its particles are about to enter, which have even corners if the
    /// particles are in cells of even parity, as the book calls it.
    fn figure(particles: &[(char, i32, i32)]) -> Vec<Cell> {
        let odd = (particles[0].1 - particles[0].2) & 1;
        let same_parity = |&(_, column, row): &(char, i32, i32)| (column - row) & 1 == odd;
        assert!(particles.iter().all(same_parity), "particles that never meet");
        sites(particles).iter().map(|&(x, y)| (x + odd, y + odd)).collect()
    }

    /// So many cells east and north in the book, as a displacement here.
    fn moved(east: i32, north: i32) -> (i32, i32) {
        (east + north, east - north)
    }

    /// The period of a pattern and how far it moves in it, as it lies.
    fn runs(number: &str, cells: &[Cell]) -> Option<(u32, (i32, i32))> {
        match Analyser::new(&rule(number)).fate(cells, 0) {
            Fate::Returns { period, displacement } => Some((period, displacement)),
            _ => None,
        }
    }

    #[test]
    fn the_figures_do_what_the_book_says() {
        // Figs. 5.12, 5.13 and 5.41: the rotor, the blinker and the glider-12, which goes one
        // cell north-east in its period.
        let rotor = figure(&[('L', 3, 3)]);
        let blinker = figure(&[('T', 2, 3), ('B', 3, 2)]);
        let glider_12 = figure(&[('L', 2, 3), ('T', 2, 3), ('L', 3, 4), ('T', 3, 4)]);
        for number in ["01c5ef", "01caef"] {
            assert_eq!(runs(number, &rotor), Some((4, STILL)), "{number}");
            assert_eq!(runs(number, &blinker), Some((2, STILL)), "{number}");
            assert_eq!(runs(number, &glider_12), Some((12, moved(1, 1))), "{number}");
        }
        // Fig. 5.44: five particles going north at a third of the speed of light.
        let ship = figure(&[('R', 3, 2), ('T', 2, 3), ('L', 2, 3), ('R', 2, 3), ('T', 3, 4)]);
        assert_eq!(runs("016a7f", &ship), Some((3, moved(0, 1))));
        // Figs. 5.47 and 5.49: the glider-3 and the glider-5, both going east.
        let glider_3 = figure(&[('R', 1, 2), ('B', 2, 1), ('R', 2, 1)]);
        let glider_5 =
            figure(&[('L', 2, 1), ('B', 2, 1), ('T', 1, 2), ('R', 1, 2), ('B', 1, 2), ('L', 2, 3), ('T', 2, 3)]);
        for number in ["0945df", "09457f"] {
            assert_eq!(runs(number, &glider_3), Some((3, moved(1, 0))), "{number}");
        }
        assert_eq!(runs("098a7f", &glider_5), Some((5, moved(1, 0))));
        // Fig. 2.9: a cell with a particle in every part has period 6.
        let full = figure(&[('T', 3, 3), ('R', 3, 3), ('B', 3, 3), ('L', 3, 3)]);
        assert_eq!(runs("0945df", &full), Some((6, STILL)));
        // Fig. 5.39: a lone particle flies at the speed of light.
        assert_eq!(runs("02c5bf", &figure(&[('R', 1, 1)])), Some((1, moved(1, 0))));
    }

    #[test]
    fn the_rotor_turns_clockwise() {
        // Fig. 5.12, frame by frame.
        let frames = [('L', 3, 3), ('T', 2, 3), ('R', 2, 2), ('B', 3, 2), ('L', 3, 3)];
        let on_grid = |particle| {
            let (x, y) = sites(&[particle])[0];
            ((x + 16) as usize, (y + 16) as usize)
        };
        let mut universe = Universe::new(32, 32, rule("01c5ef"));
        let (x, y) = on_grid(frames[0]);
        universe.set(x, y, true);
        for frame in frames {
            let (x, y) = on_grid(frame);
            assert!(universe.get(x, y), "{frame:?}");
            assert_eq!(universe.population(), 1);
            universe.step(true);
        }
    }

    #[test]
    fn spaceships_have_the_periods_the_book_lists() {
        let period = |number: &str, rle: &str| {
            let motion = Analyser::new(&rule(number)).analyse(&from_rle(rle).unwrap(), 0).unwrap();
            assert_ne!(motion.heading(), Heading::Still, "{rle}");
            motion.period
        };
        // Fig. 5.42 draws spaceships of ESPCA-01caef too small to read off. These are the
        // ones a search of small patterns turns up, with the periods of the figure.
        for (rle, expected) in [("2o2$2o", 12), ("2o2$obo", 28), ("$bo$2o2$o", 44), ("b2o$bo$bo", 61), ("2ob2o", 368)] {
            assert_eq!(period("01caef", rle), expected, "{rle}");
        }
        // Likewise Fig. 5.45 for ESPCA-016a7f, and the glider-10 of Fig. 5.54.
        assert_eq!(period("016a7f", "o2bo$b3o"), 829);
        assert_eq!(period("098aef", "2o$bo$bo"), 10);
        // Not in the book: a spaceship of period 17, in ESPCA-098a7f as well.
        assert_eq!(period("098aef", "obo$b2o"), 17);
        assert_eq!(period("098a7f", "obo$b2o"), 17);
    }

    #[test]
    fn a_cell_grows_into_a_disk() {
        // Sec. 5.6.2: ESPCA-0925bf "generates disk-like patterns that are very close to true
        // disks". How far the pattern reaches from where it began is then the same in every
        // direction, where for a square it would differ by a factor of √2.
        let mut universe = Universe::new(512, 512, rule("0925bf"));
        universe.set(256, 256, true);
        universe.step_by(400);
        let cells: Vec<(f32, f32)> = (0..512 * 512)
            .filter(|&i| universe.cells()[i] != 0)
            .map(|i| ((i % 512) as f32 - 256.0, (i / 512) as f32 - 256.0))
            .collect();
        let reach = |direction: u8| {
            let (sin, cos) = (direction as f32 * std::f32::consts::TAU / 16.0).sin_cos();
            cells.iter().map(|&(x, y)| x * cos + y * sin).fold(f32::MIN, f32::max)
        };
        let reaches: Vec<f32> = (0..16).map(reach).collect();
        let nearest = reaches.iter().copied().fold(f32::MAX, f32::min);
        let farthest = reaches.iter().copied().fold(f32::MIN, f32::max);
        assert!(nearest > 150.0, "it has grown to {nearest}");
        assert!(farthest / nearest < 1.1, "from {nearest} to {farthest}");
    }

    #[test]
    fn guns_fire_as_often_as_the_book_says() {
        // Sec. 5.5.1: in ESPCA-094x7f a single particle sends out four glider-3's every 8
        // steps, and backwards in time it does the same.
        for number in ["09457f", "094a7f"] {
            for direction in [1, -1] {
                let mut universe = Universe::new(256, 256, rule(number));
                universe.set(128, 128, true);
                universe.step_by(4 * direction);
                let mut population = universe.population();
                for _ in 0..8 {
                    universe.step_by(8 * direction);
                    assert_eq!(universe.population(), population + 4 * 3, "{number}");
                    population += 4 * 3;
                }
            }
        }
        // Fig. 2.11: in ESPCA-0945df this pattern sends out two glider-3's every 10 steps. Its
        // particles are about to fill two cells, which is two full blocks side by side.
        let seed = figure(&[
            ('B', 5, 2),
            ('R', 4, 3),
            ('B', 4, 3),
            ('L', 6, 3),
            ('R', 3, 4),
            ('L', 5, 4),
            ('T', 5, 4),
            ('T', 4, 5),
        ]);
        assert_eq!(to_rle(&settled(&seed)), "4o$4o");
        let mut universe = Universe::new(256, 256, rule("0945df"));
        for (x, y) in settled(&seed) {
            universe.set((128 + x) as usize, (128 + y) as usize, true);
        }
        universe.step(true);
        let mut population = universe.population();
        for _ in 0..8 {
            universe.step_by(10);
            assert_eq!(universe.population(), population + 2 * 3);
            population += 2 * 3;
        }
    }
}
