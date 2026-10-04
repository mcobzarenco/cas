//! Counting spaceships by kind.
//!
//! A universe that is catching hands over the small patterns that reached its edge as
//! [`Departure`]s. A [`Census`] runs each one alone until it repeats ([`Analyser`]); those that
//! travel are spaceships, and are counted under their canonical form.
//!
//! A slow spaceship takes long to repeat, longer than a census that is to keep up with the
//! catches can wait. What has not repeated in its time can be handed back uncounted
//! ([`Census::record_or_defer`]), looked at by someone with more patience ([`identify`]), on
//! another thread if need be, and counted then ([`Census::count`]).

use std::collections::HashMap;

use crate::{
    pattern::{Analyser, Cell, Fate, Heading, Motion, settled, way},
    rules::BlockRule,
    universe::{Departure, PATTERN_REACH},
};

/// When this many shapes are remembered, the memory starts over.
const REMEMBERED: usize = 100_000;

/// A spaceship as it was caught: which kind it is, and which of the eight ways it was flying.
type Caught = (usize, Option<usize>);

/// The spaceships of one rule, as far as they have been caught.
pub struct Census {
    analyser: Analyser,
    kinds: Vec<Kind>,
    /// Shapes that were caught before, each with the vacuum's phase, and the spaceships they
    /// turned out to be. Most catches are repeats.
    seen: HashMap<(Vec<Cell>, usize), Vec<Caught>>,
    ships: u64,
    others: u64,
}

/// One kind of spaceship, how often it was caught, and how often flying each of the eight
/// [`WAYS`](crate::pattern::WAYS). A kind flies the ways the rule's own turns and mirrors take
/// it: all four of its sort under a rule that looks the same after a quarter turn, and one
/// alone under a rule with no symmetry.
pub struct Kind {
    pub motion: Motion,
    pub count: u64,
    pub ways: [u64; 8],
}

/// What a catch turned out to be: its spaceships, each as the motion it is filed under and
/// how far it moves in a period as it lies; and whether something about it was left
/// undecided, so that a longer look might find more.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub ships: Vec<(Motion, (i32, i32))>,
    pub undecided: bool,
}

/// Finds out what was caught, as far as `analyser` follows it. The catch as a whole first: a
/// spaceship, or several flying side by side. If it is none, each of its lots of cells that
/// lie close together on its own: where there is little else around, the universe takes what
/// lies a little apart as one pattern, which a loose spaceship is, and a spaceship with a
/// bystander is not.
pub fn identify(analyser: &Analyser, cells: &[Cell], phase: usize) -> Found {
    let whole = ships_of(analyser, cells, phase);
    if let Some(ships) = whole.clone().filter(|ships| !ships.is_empty()) {
        return Found { ships, undecided: false };
    }
    let mut found = Found { ships: Vec::new(), undecided: whole.is_none() };
    let lots = close_together(cells);
    if lots.len() > 1 {
        for lot in &lots {
            match ships_of(analyser, lot, phase) {
                Some(ships) => found.ships.extend(ships),
                None => found.undecided = true,
            }
        }
    }
    found
}

/// The spaceships a pattern is, taken as one: none if it does not travel, and more than one
/// if it is several flying side by side. Not known (`None`) if it did not repeat in time.
fn ships_of(analyser: &Analyser, cells: &[Cell], phase: usize) -> Option<Vec<(Motion, (i32, i32))>> {
    match analyser.motion_or_fate(cells, phase) {
        Ok(whole) if whole.0.heading() != Heading::Still => {
            let parts = analyser.parts(cells, phase, whole.0.period);
            Some(match parts.len() {
                1 => vec![whole],
                _ => parts.iter().filter_map(|part| analyser.analyse_as_found(part, phase)).collect(),
            })
        }
        Err(Fate::Undecided) => None,
        // It stays where it is, or it got out of hand.
        _ => Some(Vec::new()),
    }
}

/// The cells in lots that lie close together: within the reach that makes one pattern of them
/// wherever a universe looks for one.
fn close_together(cells: &[Cell]) -> Vec<Vec<Cell>> {
    let mut lot_of: Vec<usize> = (0..cells.len()).collect();
    let near = |a: Cell, b: Cell| (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= PATTERN_REACH;
    for a in 0..cells.len() {
        for b in 0..a {
            if near(cells[a], cells[b]) && lot_of[a] != lot_of[b] {
                let (kept, gone) = (lot_of[a].min(lot_of[b]), lot_of[a].max(lot_of[b]));
                lot_of.iter_mut().filter(|lot| **lot == gone).for_each(|lot| *lot = kept);
            }
        }
    }
    let mut lots: Vec<usize> = lot_of.clone();
    lots.sort_unstable();
    lots.dedup();
    let cells_of =
        |lot: &usize| (0..cells.len()).filter(|&cell| lot_of[cell] == *lot).map(|cell| cells[cell]).collect();
    lots.iter().map(cells_of).collect()
}

impl Census {
    pub fn new(rule: &BlockRule) -> Self {
        Self::with(Analyser::new(rule))
    }

    /// A census that follows what was caught as far as `analyser` does.
    pub fn with(analyser: Analyser) -> Self {
        Self { analyser, kinds: Vec::new(), seen: HashMap::new(), ships: 0, others: 0 }
    }

    /// The kinds, in the order they were first caught.
    pub fn kinds(&self) -> &[Kind] {
        &self.kinds
    }

    /// How many spaceships were caught, of all kinds together.
    pub fn ships(&self) -> u64 {
        self.ships
    }

    /// How many catches were no spaceship: oscillators, and whatever fell apart or never
    /// repeated.
    pub fn others(&self) -> u64 {
        self.others
    }

    /// Finds out what was caught, and counts it. What does not repeat in the time the census
    /// gives it is no spaceship here.
    pub fn record(&mut self, departure: Departure) {
        if let Some(undecided) = self.record_or_defer(departure) {
            self.count(&undecided, Vec::new());
        }
    }

    /// As [`Census::record`], but a catch that has not repeated in time, and in which no
    /// spaceship was found either, comes back uncounted: a longer look may tell what it is
    /// ([`identify`], with an analyser of more patience), to be counted then
    /// ([`Census::count`]).
    pub fn record_or_defer(&mut self, departure: Departure) -> Option<Departure> {
        let shape = (settled(&departure.cells), departure.phase);
        if !self.seen.contains_key(&shape) {
            let found = identify(&self.analyser, &shape.0, shape.1);
            if found.undecided && found.ships.is_empty() {
                return Some(departure);
            }
            self.remember(shape.clone(), found.ships);
        }
        self.tally(&shape);
        None
    }

    /// Counts a catch as the spaceships it was found to be, by whoever looked: as none of
    /// them if `ships` is empty.
    pub fn count(&mut self, departure: &Departure, ships: Vec<(Motion, (i32, i32))>) {
        let shape = (settled(&departure.cells), departure.phase);
        if !self.seen.contains_key(&shape) {
            self.remember(shape.clone(), ships);
        }
        self.tally(&shape);
    }

    /// Files the spaceships a shape was found to be, and remembers them for the next time it
    /// is caught.
    fn remember(&mut self, shape: (Vec<Cell>, usize), ships: Vec<(Motion, (i32, i32))>) {
        let kinds = ships.into_iter().map(|(motion, (dx, dy))| (self.file(motion), way(dx, dy))).collect();
        if self.seen.len() >= REMEMBERED {
            self.seen.clear();
        }
        self.seen.insert(shape, kinds);
    }

    /// One more catch of a shape that is remembered.
    fn tally(&mut self, shape: &(Vec<Cell>, usize)) {
        let kinds = &self.seen[shape];
        for &(kind, way) in kinds {
            self.kinds[kind].count += 1;
            if let Some(way) = way {
                self.kinds[kind].ways[way] += 1;
            }
        }
        self.ships += kinds.len() as u64;
        self.others += kinds.is_empty() as u64;
    }

    /// The kind with this canonical form, new if need be.
    fn file(&mut self, motion: Motion) -> usize {
        let known = self.kinds.iter().position(|kind| kind.motion.canonical == motion.canonical);
        known.unwrap_or_else(|| {
            self.kinds.push(Kind { motion, count: 0, ways: [0; 8] });
            self.kinds.len() - 1
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::from_rle;

    fn departure(rle: &str) -> Departure {
        Departure { cells: from_rle(rle).unwrap(), phase: 0 }
    }

    fn census() -> Census {
        Census::new(&"single-rotation".parse().unwrap())
    }

    #[test]
    fn ships_are_counted_by_kind_whatever_way_they_fly() {
        let mut census = census();
        // The lightest ship twice, the second one flying up, and a diagonal one.
        census.record(departure("$2o2$2o"));
        census.record(Departure { cells: vec![(1, 0), (1, 1), (3, 0), (3, 1)], phase: 0 });
        census.record(departure("2bo$obo$o"));
        assert_eq!((census.ships(), census.others(), census.kinds().len()), (3, 0, 2));
        assert_eq!(census.kinds()[0].count, 2);
        assert_eq!(census.kinds()[0].motion.displacement, (2, 0));
        assert_eq!(census.kinds()[1].motion.period, 48);
        // The kind is filed flying right; one was caught flying right, and one flying up. The
        // diagonal one was going down and to the right.
        assert_eq!(census.kinds()[0].ways, [1, 0, 1, 0, 0, 0, 0, 0]);
        assert_eq!(census.kinds()[1].ways, [0, 0, 0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn ships_flying_side_by_side_are_counted_each() {
        let mut census = census();
        // The lightest ship, and the same again six rows further down.
        let mut cells = from_rle("$2o2$2o").unwrap();
        cells.extend(from_rle("$2o2$2o").unwrap().iter().map(|&(x, y)| (x, y + 6)));
        for _ in 0..2 {
            census.record(Departure { cells: cells.clone(), phase: 0 });
        }
        assert_eq!((census.ships(), census.others(), census.kinds().len()), (4, 0, 1));
        assert_eq!(census.kinds()[0].motion.canonical.len(), 4);
        assert_eq!(census.kinds()[0].ways, [0, 0, 4, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn a_slow_spaceship_is_known_to_whoever_waits_for_it() {
        // Seven cells that are back, four cells further down and to the right, after 13 774
        // generations: longer than a census follows a catch.
        let rule: BlockRule = "15,7,6,3,11,12,4,8,14,13,5,9,10,2,1,0".parse().unwrap();
        let slow = departure("4bo$3bo2$5bobo2$bo2$5bobo");
        let mut hasty = Census::new(&rule);
        hasty.record(slow.clone());
        assert_eq!((hasty.ships(), hasty.others()), (0, 1), "no spaceship to a census that cannot wait");

        // Handed back instead, it is not counted until someone has had a longer look.
        let mut census = Census::new(&rule);
        let handed_back = census.record_or_defer(slow.clone()).expect("it does not repeat in time");
        assert_eq!((census.ships(), census.others()), (0, 0));
        let mut patient = Analyser::new(&rule);
        patient.max_generations = 20_000;
        let found = identify(&patient, &handed_back.cells, handed_back.phase);
        assert!(!found.undecided);
        assert_eq!(found.ships.len(), 1);
        assert_eq!((found.ships[0].0.period, found.ships[0].1), (13_774, (4, 4)));
        census.count(&handed_back, found.ships);
        assert_eq!((census.ships(), census.others(), census.kinds().len()), (1, 0, 1));
        // The same shape again is known at once.
        assert_eq!(census.record_or_defer(slow), None);
        assert_eq!((census.ships(), census.kinds()[0].count), (2, 2));
        // What a longer look does not make a spaceship of either is none.
        let lone = departure("o");
        census.count(&lone, Vec::new());
        assert_eq!((census.ships(), census.others()), (2, 1));
    }

    #[test]
    fn a_spaceship_and_a_bystander_caught_as_one_count_as_the_spaceship() {
        let mut census = census();
        // The lightest ship, flying right, and a lone cell nine columns behind it: too far
        // to be of one pattern, near enough to be taken along where little else is around.
        let mut cells = from_rle("$2o2$2o").unwrap();
        cells.push((-9, 2));
        assert_eq!(close_together(&cells).len(), 2);
        census.record(Departure { cells, phase: 0 });
        assert_eq!((census.ships(), census.others(), census.kinds().len()), (1, 0, 1));
        assert_eq!(census.kinds()[0].motion.displacement, (2, 0));
        // Two lone cells that far apart are two things that stay.
        census.record(Departure { cells: vec![(0, 0), (9, 0)], phase: 0 });
        assert_eq!((census.ships(), census.others()), (1, 1));
    }

    #[test]
    fn what_does_not_travel_is_not_a_ship() {
        let mut census = census();
        census.record(departure("o"));
        census.record(departure("b2o$b2o"));
        census.record(departure("o"));
        assert_eq!((census.ships(), census.others(), census.kinds().len()), (0, 3, 0));
        assert_eq!(census.seen.len(), 2, "the second lone cell was recognised");
    }
}
