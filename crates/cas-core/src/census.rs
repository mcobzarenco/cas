//! Counting spaceships by kind.
//!
//! A universe that is catching hands over the small patterns that reached its edge as
//! [`Departure`]s. A [`Census`] runs each one alone until it repeats ([`Analyser`]); those that
//! travel are spaceships, and are counted under their canonical form.

use std::collections::HashMap;

use crate::{
    pattern::{Analyser, Cell, Heading, Motion, settled, way},
    rules::BlockRule,
    universe::Departure,
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

    /// Finds out what was caught, and counts it.
    pub fn record(&mut self, departure: Departure) {
        let shape = (settled(&departure.cells), departure.phase);
        if !self.seen.contains_key(&shape) {
            let kinds = self.spaceships(&shape.0, shape.1);
            if self.seen.len() >= REMEMBERED {
                self.seen.clear();
            }
            self.seen.insert(shape.clone(), kinds);
        }
        let kinds = &self.seen[&shape];
        for &(kind, way) in kinds {
            self.kinds[kind].count += 1;
            if let Some(way) = way {
                self.kinds[kind].ways[way] += 1;
            }
        }
        self.ships += kinds.len() as u64;
        self.others += kinds.is_empty() as u64;
    }

    /// The kinds of the spaceships a shape consists of, each with the way it flies: none if
    /// the shape does not travel, and more than one if it is several flying side by side.
    fn spaceships(&mut self, cells: &[Cell], phase: usize) -> Vec<Caught> {
        let travels = |(motion, _): &(Motion, (i32, i32))| motion.heading() != Heading::Still;
        let Some(whole) = self.analyser.analyse_as_found(cells, phase).filter(travels) else {
            return Vec::new();
        };
        let parts = self.analyser.parts(cells, phase, whole.0.period);
        let found = match parts.len() {
            1 => vec![whole],
            _ => parts.iter().filter_map(|part| self.analyser.analyse_as_found(part, phase)).collect(),
        };
        found.into_iter().map(|(motion, (dx, dy))| (self.file(motion), way(dx, dy))).collect()
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
    fn what_does_not_travel_is_not_a_ship() {
        let mut census = census();
        census.record(departure("o"));
        census.record(departure("b2o$b2o"));
        census.record(departure("o"));
        assert_eq!((census.ships(), census.others(), census.kinds().len()), (0, 3, 0));
        assert_eq!(census.seen.len(), 2, "the second lone cell was recognised");
    }
}
