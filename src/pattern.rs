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

use std::cmp::Reverse;

use crate::rules::{
    BlockRule, anti_transpose, flip, mirror, rotate_180, rotate_ccw, rotate_cw, transpose,
};

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

/// Recognises patterns of one rule.
pub struct Analyser {
    /// The rule relative to its vacuum, a table for each generation of the vacuum's cycle.
    tables: Vec<BlockRule>,
    /// The orientations under which the rule looks the same: a pattern turned or flipped by
    /// one of them is the same pattern, travelling another way.
    orientations: Vec<Orientation>,
    /// Where to stop: a pattern that has not repeated after this many generations, or has
    /// grown beyond this many cells or this extent, is not recognised.
    pub max_generations: u32,
    pub max_cells: usize,
    pub max_extent: i32,
}

impl Analyser {
    pub fn new(rule: &BlockRule) -> Self {
        Self {
            tables: rule.relative_to_vacuum(),
            orientations: ORIENTATIONS
                .into_iter()
                .filter(|orientation| rule.commutes_with(orientation.block))
                .collect(),
            max_generations: 8192,
            max_cells: 256,
            max_extent: 256,
        }
    }

    /// Runs the pattern alone until it repeats. `phase` says where the vacuum was in its cycle
    /// when the pattern was found.
    ///
    /// The canonical form is chosen among the forms the pattern takes through its period, each
    /// time the vacuum starts its cycle, and every orientation the rule allows, by a fixed
    /// order: the one travelling furthest right, then furthest down, then with the smallest
    /// bounding box, then first in reading order of its cells.
    pub fn analyse(&self, cells: &[Cell], phase: usize) -> Option<Motion> {
        let (period, moved, phases) = self.run(cells, phase)?;
        let (canonical, displacement) = phases
            .iter()
            .flat_map(|form| self.orientations.iter().map(move |o| reorient(form, moved, o)))
            .min_by(|(a, a_moved), (b, b_moved)| {
                let reading = |cells: &[Cell]| cells.iter().map(|&(x, y)| (y, x)).collect::<Vec<_>>();
                (Reverse(a_moved), area(a), reading(a)).cmp(&(Reverse(b_moved), area(b), reading(b)))
            })?;
        Some(Motion {
            period,
            displacement,
            canonical,
        })
    }

    /// The pattern's period, how far it moves in it as found, and its form at the start of
    /// every cycle of the vacuum along the way.
    fn run(&self, cells: &[Cell], mut phase: usize) -> Option<(u32, (i32, i32), Vec<Vec<Cell>>)> {
        if cells.is_empty() {
            return None;
        }
        let mut pattern = cells.to_vec();
        settle(&mut pattern);
        // Forms are only comparable at the same point of the vacuum's cycle: go to its start.
        while phase != 0 {
            advance(&mut pattern, self.tables[phase].table());
            phase = (phase + 1) % self.tables.len();
        }
        let mut phases = vec![pattern.clone()];
        let mut moved = (0, 0);
        let mut generation = 0;
        loop {
            for table in &self.tables {
                let (dx, dy) = advance(&mut pattern, table.table());
                moved = (moved.0 + dx, moved.1 + dy);
            }
            generation += self.tables.len() as u32;
            if pattern == phases[0] {
                return Some((generation, moved, phases));
            }
            let (width, height) = extent(&pattern);
            if generation >= self.max_generations
                || pattern.len() > self.max_cells
                || width.max(height) > self.max_extent
            {
                return None;
            }
            phases.push(pattern.clone());
        }
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
            }
            quiet = count == before;
        }
        let mut parts: Vec<(usize, Vec<Cell>)> = Vec::new();
        for (index, &cell) in cells.iter().enumerate() {
            let group = root(&mut groups, index);
            match parts.iter_mut().find(|part| part.0 == group) {
                Some(part) => part.1.push(cell),
                None => parts.push((group, vec![cell])),
            }
        }
        parts.into_iter().map(|part| part.1).collect()
    }
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
    (0..4)
        .filter(move |bit| (state >> bit) & 1 == 1)
        .map(move |bit| (2 * x + (bit & 1) - 1, 2 * y + (bit >> 1) - 1))
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

/// Width and height of the bounding box.
fn extent(cells: &[Cell]) -> (i32, i32) {
    let span = |values: &mut dyn Iterator<Item = i32>| {
        let (min, max) = values.fold((i32::MAX, i32::MIN), |(min, max), v| (min.min(v), max.max(v)));
        if min > max { 0 } else { max - min + 1 }
    };
    (
        span(&mut cells.iter().map(|cell| cell.0)),
        span(&mut cells.iter().map(|cell| cell.1)),
    )
}

fn area(cells: &[Cell]) -> i32 {
    let (width, height) = extent(cells);
    width * height
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
        sim::{Rng, Universe},
    };

    fn rule(name: &str) -> BlockRule {
        name.parse().unwrap()
    }

    fn analyse(name: &str, rle: &str) -> Motion {
        Analyser::new(&rule(name))
            .analyse(&from_rle(rle).unwrap(), 0)
            .unwrap()
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
                let Some((period, ..)) = analyser.run(&cells, 0) else {
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
                for orientation in &analyser.orientations {
                    let (turned, _) = reorient(&pattern, (0, 0), orientation);
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
    fn orientations_do_to_cells_what_they_do_to_blocks() {
        for orientation in ORIENTATIONS {
            for state in 0..16u8 {
                let cells = (0..4).filter(|bit| (state >> bit) & 1 == 1).map(|bit| (bit & 1, bit >> 1));
                let turned = cells
                    .map(|cell| (orientation.cell)(cell))
                    .fold(0, |turned, (x, y)| turned | 1 << (x + 2 * y));
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
                let Some((period, moved, _)) = analyser.run(&cells, 0) else {
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
