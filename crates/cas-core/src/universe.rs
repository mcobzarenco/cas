//! The universe: a toroidal grid of cells plus the machinery to step it forwards and backwards.

use rayon::prelude::*;

use crate::rules::BlockRule;

/// SplitMix64: tiny, fast and deterministic, which is all a random soup needs.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::resource::Resource))]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The grid of cells. It wraps around, unless its border is open.
///
/// What is stored, one `0`/`1` byte per cell and row by row, is how the world differs from
/// the vacuum: for a rule that leaves empty space alone, simply the live cells. That is the
/// picture one wants to see, paint and count, and it steps under the rule taken relative to its
/// vacuum ([`BlockRule::relative_to_vacuum`]). The cells of the automaton proper are these XOR
/// the vacuum, which only the unhidden view needs: see [`Universe::vacuum`].
#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::resource::Resource))]
pub struct Universe {
    pub width: usize,
    pub height: usize,
    cells: Vec<u8>,
    scratch: Vec<u8>,
    /// How many steps we are from the initial condition; the partition offset follows its parity.
    pub generation: i64,
    rule: BlockRule,
    /// The vacuum's cycle and, for each of its generations, the kernels that step the
    /// difference from it forwards and backwards.
    vacuum: Vec<u8>,
    kernels: Vec<(Kernel, Kernel)>,
    /// Where in its cycle the vacuum is.
    phase: usize,
    /// With an open border, whatever reaches the edge of the grid leaves the world.
    pub open_border: bool,
    /// Small patterns that reach the edge are taken out of the world and kept as
    /// [`Departure`]s, whatever the border otherwise does.
    pub catching: bool,
    departures: Vec<Departure>,
}

/// A small pattern that was caught at the edge, as it was at that moment: its cells relative
/// to a corner of the blocks the next step would have rewritten, and where the vacuum was in
/// its cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Departure {
    pub cells: Vec<(i32, i32)>,
    pub phase: usize,
}

/// Live cells this close to each other, along both axes, belong to the same pattern.
const PATTERN_REACH: i32 = 4;
/// Something with this many cells or more is debris, not a pattern.
pub const PATTERN_CELLS: usize = 20;

impl Universe {
    pub fn new(width: usize, height: usize, rule: BlockRule) -> Self {
        Self::check_size(width, height);
        let n = width * height;
        let mut universe = Self {
            width,
            height,
            cells: vec![0; n],
            scratch: vec![0; n],
            generation: 0,
            rule: BlockRule::identity(),
            vacuum: Vec::new(),
            kernels: Vec::new(),
            phase: 0,
            open_border: false,
            catching: false,
            departures: Vec::new(),
        };
        universe.install(rule);
        universe
    }

    fn check_size(width: usize, height: usize) {
        assert!(
            width >= 2 && height >= 2 && width.is_multiple_of(2) && height.is_multiple_of(2),
            "a Margolus grid needs even dimensions, got {width}×{height}"
        );
    }

    pub fn rule(&self) -> &BlockRule {
        &self.rule
    }

    /// Switches rule in place. The cells and the generation counter are kept; the picture is
    /// from now on read against the new rule's vacuum, at the start of its cycle.
    pub fn set_rule(&mut self, rule: BlockRule) {
        if rule != self.rule {
            self.install(rule);
        }
    }

    fn install(&mut self, rule: BlockRule) {
        self.vacuum = rule.vacuum_cycle();
        self.kernels = rule
            .relative_to_vacuum()
            .iter()
            .map(|table| {
                (
                    Kernel::new(table.table_for(true)),
                    Kernel::new(table.table_for(false)),
                )
            })
            .collect();
        self.phase = 0;
        self.rule = rule;
    }

    /// Gives the grid another size. What is on it stays where it is, seen from the middle, as
    /// far as it still fits.
    pub fn resize(&mut self, width: usize, height: usize) {
        Self::check_size(width, height);
        if (width, height) == (self.width, self.height) {
            return;
        }
        // Rows and columns come and go on both sides alike, but only in pairs: the pattern
        // keeps its place among the blocks.
        let margin = |old: usize, new: usize| (old.abs_diff(new) / 2) & !1;
        let (dx, dy) = (margin(self.width, width), margin(self.height, height));
        let (from_x, to_x) = if width < self.width { (dx, 0) } else { (0, dx) };
        let (from_y, to_y) = if height < self.height { (dy, 0) } else { (0, dy) };
        let kept = width.min(self.width);
        let mut cells = vec![0; width * height];
        for y in 0..height.min(self.height) {
            let from = (from_y + y) * self.width + from_x;
            let to = (to_y + y) * width + to_x;
            cells[to..to + kept].copy_from_slice(&self.cells[from..from + kept]);
        }
        self.cells = cells;
        self.scratch = vec![0; width * height];
        self.width = width;
        self.height = height;
    }

    /// The vacuum right now: the state of every block of the current partition in an empty
    /// world. For most rules, and at the start of every cycle, that is 0: all cells dead.
    pub fn vacuum(&self) -> u8 {
        self.vacuum[self.phase]
    }

    /// How far the vacuum is into its cycle.
    pub fn phase(&self) -> usize {
        self.phase
    }

    /// The cells, row by row, one `0`/`1` byte each.
    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        self.cells[y * self.width + x] != 0
    }

    pub fn set(&mut self, x: usize, y: usize, alive: bool) {
        self.cells[y * self.width + x] = alive as u8;
    }

    pub fn clear(&mut self) {
        self.cells.fill(0);
        self.reset_clock();
    }

    pub fn randomize(&mut self, density: f32, rng: &mut Rng) {
        for cell in &mut self.cells {
            *cell = (rng.next_f32() < density) as u8;
        }
        self.reset_clock();
    }

    /// Clears the grid and fills a random square in the middle, a quarter of the grid wide.
    /// Nicer than a uniform soup for watching spaceships leave the mess.
    pub fn randomize_blob(&mut self, density: f32, rng: &mut Rng) {
        self.cells.fill(0);
        let side = (self.width.min(self.height) / 4).max(2);
        let x0 = (self.width - side) / 2;
        let y0 = (self.height - side) / 2;
        for y in y0..y0 + side {
            for cell in &mut self.cells[y * self.width + x0..][..side] {
                *cell = (rng.next_f32() < density) as u8;
            }
        }
        self.reset_clock();
    }

    /// A fresh pattern is generation 0, at the start of the vacuum's cycle.
    fn reset_clock(&mut self) {
        self.generation = 0;
        self.phase = 0;
    }

    pub fn population(&self) -> usize {
        // Chunks short enough for `u32` sums, which vectorise far better than a `usize` one.
        self.cells
            .chunks(1 << 16)
            .map(|chunk| chunk.iter().map(|&cell| cell as u32).sum::<u32>() as usize)
            .sum()
    }

    /// Partition offset of the forward step that takes generation `g` to `g + 1`.
    fn offset_at(generation: i64) -> usize {
        generation.rem_euclid(2) as usize
    }

    /// Offset (0 or 1) of the current partition: the blocks a forward step rewrites next.
    /// A backward step undoes the previous generation's partition, i.e. the other one.
    pub fn partition_offset(&self) -> usize {
        Self::offset_at(self.generation)
    }

    /// One generation forwards or backwards. Backwards means undoing the forward step that led
    /// here: the inverse table with the partition that step used.
    pub fn step(&mut self, forward: bool) {
        let cycle = self.vacuum.len();
        if forward {
            self.apply(true);
            self.generation += 1;
            self.phase = (self.phase + 1) % cycle;
        } else {
            self.generation -= 1;
            self.phase = (self.phase + cycle - 1) % cycle;
            self.apply(false);
        }
        if self.open_border || self.catching {
            self.sweep_edge();
        }
    }

    /// Positive `steps` go forwards in time, negative ones backwards.
    pub fn step_by(&mut self, steps: i64) {
        for _ in 0..steps.unsigned_abs() {
            self.step(steps > 0);
        }
    }

    /// Rewrites the blocks of the current partition, at the current place in the vacuum's cycle.
    fn apply(&mut self, forward: bool) {
        let (forwards, backwards) = &self.kernels[self.phase];
        let kernel = if forward { forwards } else { backwards };
        let offset = self.partition_offset();
        let parallel = self.cells.len() >= PARALLEL_CELLS;
        step_blocks(&self.cells, &mut self.scratch, self.width, kernel, offset, parallel);
        std::mem::swap(&mut self.cells, &mut self.scratch);
    }

    /// Deals with what reached the edge: the live cells in the first row and the first column.
    /// On a torus those two lines are the whole edge, and nothing can cross them unseen, since
    /// nothing moves faster than a cell per step. A small pattern leaves whole, and is kept if
    /// patterns are being caught. Of anything bigger an open border takes the cells that touch
    /// it, and a closed one nothing.
    fn sweep_edge(&mut self) {
        let (width, height) = (self.width, self.height);
        // Nearly always there is nothing on the edge, and finding that out has to cost next
        // to nothing: this runs after every generation.
        let row = self.cells[..width].iter().fold(0, |any, cell| any | cell);
        let column = self.cells.chunks_exact(width).fold(0, |any, row| any | row[0]);
        if row | column == 0 {
            return;
        }
        let edge = (0..width).map(|x| (x, 0)).chain((1..height).map(|y| (0, y)));
        // Debris is recognised once: a live cell within reach of it is part of it, and that
        // is how many of the cells to come along the edge still are.
        let mut reach = 0;
        for (x, y) in edge {
            let near_debris = reach > 0;
            reach = (reach - 1).max(0);
            if self.cells[y * width + x] == 0 {
                continue;
            }
            if !near_debris && let Some(departure) = self.take_pattern(x, y) {
                if self.catching {
                    self.departures.push(departure);
                }
                continue;
            }
            reach = PATTERN_REACH;
            if self.open_border {
                self.cells[y * width + x] = 0;
            }
        }
    }

    /// Takes the pattern that the live cell at `(x, y)` belongs to out of the world, if it is
    /// a small one.
    fn take_pattern(&mut self, x: usize, y: usize) -> Option<Departure> {
        let (width, height) = (self.width as i32, self.height as i32);
        // Coordinates do not wrap here, so that a pattern lying across the edge keeps its
        // shape; only looking a cell up wraps them.
        let wrap = |v: i32, size: i32| if (0..size).contains(&v) { v } else { v.rem_euclid(size) };
        let index = |(x, y): (i32, i32)| (wrap(y, height) * width + wrap(x, width)) as usize;

        // Flood fill. Cells are taken out as they are found, which also marks them as seen.
        let mut pattern = vec![(x as i32, y as i32)];
        self.cells[index(pattern[0])] = 0;
        let mut visited = 0;
        while visited < pattern.len() && pattern.len() < PATTERN_CELLS {
            let (cx, cy) = pattern[visited];
            visited += 1;
            for dy in -PATTERN_REACH..=PATTERN_REACH {
                for dx in -PATTERN_REACH..=PATTERN_REACH {
                    let cell = (cx + dx, cy + dy);
                    if std::mem::take(&mut self.cells[index(cell)]) != 0 {
                        pattern.push(cell);
                    }
                }
            }
        }
        if pattern.len() >= PATTERN_CELLS {
            // Debris: it goes back.
            for &cell in &pattern {
                self.cells[index(cell)] = 1;
            }
            return None;
        }
        let offset = self.partition_offset() as i32;
        let block_corner = |v: i32| v - ((v - offset) & 1);
        let (x0, y0) = (block_corner(x as i32), block_corner(y as i32));
        Some(Departure {
            cells: pattern.iter().map(|&(x, y)| (x - x0, y - y0)).collect(),
            phase: self.phase,
        })
    }

    pub fn has_departures(&self) -> bool {
        !self.departures.is_empty()
    }

    /// The patterns caught at the edge since this was last asked.
    pub fn take_departures(&mut self) -> Vec<Departure> {
        std::mem::take(&mut self.departures)
    }
}

/// Grids smaller than this are stepped on the calling thread: handing rows to other threads
/// would cost more than the step.
const PARALLEL_CELLS: usize = 1 << 18;
/// The smallest piece of a step worth a task of its own, in cells.
const TASK_CELLS: usize = 1 << 16;

/// A rule table expanded for the stepping loop, so that a lookup yields cells ready to store.
#[derive(Clone, Debug)]
struct Kernel {
    /// Indexed by the states of two horizontally adjacent blocks, `left | right << 4`. The low
    /// half holds the four cells of their top row, the high half those of their bottom row.
    pairs: Box<[u64; 256]>,
    /// One block: its cells top-left, top-right, bottom-left, bottom-right.
    blocks: [[u8; 4]; 16],
}

impl Kernel {
    fn new(table: &[u8; 16]) -> Self {
        let blocks = table.map(|out| [out & 1, (out >> 1) & 1, (out >> 2) & 1, (out >> 3) & 1]);
        let mut pairs = Box::new([0u64; 256]);
        for (index, entry) in pairs.iter_mut().enumerate() {
            let (left, right) = (blocks[index & 15], blocks[index >> 4]);
            let top = u32::from_le_bytes([left[0], left[1], right[0], right[1]]);
            let bottom = u32::from_le_bytes([left[2], left[3], right[2], right[3]]);
            *entry = top as u64 | (bottom as u64) << 32;
        }
        Self { pairs, blocks }
    }

    fn block(&self, top: [u8; 2], bottom: [u8; 2]) -> [u8; 4] {
        let state = top[0] | (top[1] << 1) | (bottom[0] << 2) | (bottom[1] << 3);
        self.blocks[(state & 15) as usize]
    }
}

/// One Margolus step: every 2×2 block whose top-left corner sits at
/// `(offset + 2i, offset + 2j)` (wrapping) is replaced by the kernel's entry for it.
///
/// A block lives in two rows, so the grid is walked a pair of rows at a time. Pairs are
/// independent of each other, which is what lets rayon share them out.
fn step_blocks(
    cells: &[u8],
    next: &mut [u8],
    width: usize,
    kernel: &Kernel,
    offset: usize,
    parallel: bool,
) {
    debug_assert!(width.is_multiple_of(2) && cells.len().is_multiple_of(2 * width));
    debug_assert_eq!(cells.len(), next.len());

    // The shifted partition's pairs start one row down, and the pair left over wraps around
    // the torus: the last row on top, the first row below.
    let (cells, next) = if offset == 0 {
        (cells, next)
    } else {
        let (first, rest) = next.split_at_mut(width);
        let (middle, last) = rest.split_at_mut(rest.len() - width);
        let wrapping_top = &cells[cells.len() - width..];
        step_row_pair(wrapping_top, &cells[..width], last, first, kernel, offset);
        (&cells[width..cells.len() - width], middle)
    };

    let step_pair = |(cells, next): (&[u8], &mut [u8])| {
        let (top, bottom) = cells.split_at(width);
        let (next_top, next_bottom) = next.split_at_mut(width);
        step_row_pair(top, bottom, next_top, next_bottom, kernel, offset);
    };
    let pair = 2 * width;
    if parallel {
        cells
            .par_chunks_exact(pair)
            .zip(next.par_chunks_exact_mut(pair))
            .with_min_len((TASK_CELLS / pair).max(1))
            .for_each(step_pair);
    } else {
        cells
            .chunks_exact(pair)
            .zip(next.chunks_exact_mut(pair))
            .for_each(step_pair);
    }
}

/// Rewrites the blocks of one pair of rows. The first whole block starts at column `offset`;
/// with an offset of 1 the last and the first column form the block that wraps around.
fn step_row_pair(
    top: &[u8],
    bottom: &[u8],
    next_top: &mut [u8],
    next_bottom: &mut [u8],
    kernel: &Kernel,
    offset: usize,
) {
    let width = top.len();
    let whole = offset..width - offset;
    let (tops, top_rest) = top[whole.clone()].as_chunks::<8>();
    let (bottoms, bottom_rest) = bottom[whole.clone()].as_chunks::<8>();
    let (next_tops, next_top_rest) = next_top[whole.clone()].as_chunks_mut::<8>();
    let (next_bottoms, next_bottom_rest) = next_bottom[whole].as_chunks_mut::<8>();

    // Four blocks at a time: eight cells of each row in one load.
    for (((t, b), next_t), next_b) in tops.iter().zip(bottoms).zip(next_tops).zip(next_bottoms) {
        let t = u64::from_le_bytes(*t);
        let b = u64::from_le_bytes(*b);
        // Every byte becomes `top | bottom << 2`; folding each odd byte onto its left
        // neighbour then leaves one block state per 16 bits, and two per 32 bits.
        let columns = t | (b << 2);
        let blocks = (columns | (columns >> 7)) & 0x000F_000F_000F_000F;
        let pairs = blocks | (blocks >> 12);
        let left = kernel.pairs[(pairs & 0xFF) as usize];
        let right = kernel.pairs[((pairs >> 32) & 0xFF) as usize];
        *next_t = ((left & 0xFFFF_FFFF) | (right << 32)).to_le_bytes();
        *next_b = ((left >> 32) | (right & !0xFFFF_FFFF)).to_le_bytes();
    }

    // Up to three blocks are left over.
    for (((t, b), next_t), next_b) in top_rest
        .as_chunks::<2>()
        .0
        .iter()
        .zip(bottom_rest.as_chunks::<2>().0)
        .zip(next_top_rest.as_chunks_mut::<2>().0)
        .zip(next_bottom_rest.as_chunks_mut::<2>().0)
    {
        let out = kernel.block(*t, *b);
        *next_t = [out[0], out[1]];
        *next_b = [out[2], out[3]];
    }

    if offset == 1 {
        let last = width - 1;
        let out = kernel.block([top[last], top[0]], [bottom[last], bottom[0]]);
        (next_top[last], next_top[0]) = (out[0], out[1]);
        (next_bottom[last], next_bottom[0]) = (out[2], out[3]);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::rules::{PRESETS, Population, Preset};

    fn rule(id: &str) -> BlockRule {
        id.parse().unwrap()
    }

    fn soup(rule: BlockRule, seed: u64) -> Universe {
        let mut rng = Rng::new(seed);
        let mut universe = Universe::new(32, 16, rule);
        universe.randomize(0.35, &mut rng);
        universe
    }

    #[test]
    fn stepping_back_restores_the_past() {
        let mut rng = Rng::new(99);
        let random = (0..8).map(|_| BlockRule::random(|| rng.next_u64()));
        let rules: Vec<_> = PRESETS.iter().map(|preset| preset.rule()).chain(random).collect();
        for rule in rules {
            let start = soup(rule.clone(), 7);
            let mut universe = start.clone();
            universe.step_by(37);
            universe.step_by(-37);
            assert_eq!(universe.generation, 0);
            assert_eq!(universe.cells, start.cells, "{rule} is not reversible");
            // And the other way round: the past of a state leads back to it.
            universe.step_by(-20);
            universe.step_by(20);
            assert_eq!(universe.cells, start.cells, "{rule} is not reversible");
        }
    }

    #[test]
    fn a_rule_begun_a_generation_later_makes_the_same_world() {
        let mut rng = Rng::new(31);
        let random = (0..40).map(|_| BlockRule::random(|| rng.next_u64()));
        let rules: Vec<_> = PRESETS.iter().map(|preset| preset.rule()).chain(random).collect();
        for rule in rules {
            let mut world = soup(rule.clone(), 7);
            for (generation, later) in rule.begun_later().iter().enumerate().skip(1) {
                // The world so many generations on, moved up and left by as many cells: its
                // blocks are then where a world at its beginning has them.
                world.step(true);
                let (width, height) = (world.width, world.height);
                let moved = |universe: &Universe, x: usize, y: usize| {
                    universe.get((x + generation) % width, (y + generation) % height)
                };
                let mut begun = Universe::new(width, height, later.clone());
                for (x, y) in (0..height).flat_map(|y| (0..width).map(move |x| (x, y))) {
                    begun.set(x, y, moved(&world, x, y));
                }
                let mut ahead = world.clone();
                for _ in 0..12 {
                    ahead.step(true);
                    begun.step(true);
                    let same = (0..height).all(|y| (0..width).all(|x| begun.get(x, y) == moved(&ahead, x, y)));
                    assert!(same, "{rule} and {later} part ways");
                }
            }
        }
    }

    #[test]
    fn presets_other_than_the_identity_do_something() {
        for preset in &PRESETS {
            let start = soup(preset.rule(), 7);
            let mut universe = start.clone();
            universe.step_by(3);
            assert_ne!(universe.cells, start.cells, "{}: nothing happened", preset.id);
        }
    }

    #[test]
    fn population_conserving_rules_conserve_population() {
        let conserving = |preset: &&Preset| preset.rule().population() == Population::Conserved;
        for preset in PRESETS.iter().filter(conserving) {
            let mut universe = soup(preset.rule(), 5);
            let population = universe.population();
            for _ in 0..20 {
                universe.step(true);
                assert_eq!(universe.population(), population, "{}", preset.id);
            }
        }
    }

    #[test]
    fn single_rotation_conserves_population() {
        let mut universe = soup(rule("single-rotation"), 3);
        let population = universe.population();
        universe.step_by(100);
        assert_eq!(universe.population(), population);
    }

    #[test]
    fn critters_conserves_what_differs_from_the_vacuum() {
        let mut universe = soup(rule("critters"), 11);
        let population = universe.population();
        for _ in 0..25 {
            universe.step(true);
            assert_eq!(universe.population(), population);
        }
    }

    #[test]
    fn the_vacuum_cycles_beside_the_cells() {
        // An empty world stays empty in store, while its vacuum goes all alive and back.
        let mut universe = Universe::new(8, 8, rule("critters"));
        assert_eq!(universe.vacuum(), 0);
        universe.step(true);
        assert_eq!((universe.population(), universe.vacuum()), (0, 15));
        universe.step(true);
        assert_eq!((universe.population(), universe.vacuum()), (0, 0));
        universe.step(false);
        assert_eq!(universe.vacuum(), 15, "stepping back takes the vacuum back too");
    }

    #[test]
    fn the_picture_survives_a_rule_switch() {
        let mut universe = soup(rule("critters"), 21);
        universe.step_by(3);
        assert_eq!(universe.vacuum(), 15);
        let picture = universe.cells().to_vec();
        universe.set_rule(rule("single-rotation"));
        assert_eq!(universe.cells(), picture);
        assert_eq!(universe.vacuum(), 0, "the new rule reads it against its own vacuum");
        assert_eq!(universe.generation, 3, "the clock is not touched");
    }

    #[test]
    fn a_new_pattern_starts_the_vacuum_cycle_afresh() {
        let mut rng = Rng::new(1);
        let mid_cycle = || {
            let mut universe = soup(rule("critters"), 4);
            universe.step(true);
            universe
        };
        let (mut cleared, mut soup, mut blob) = (mid_cycle(), mid_cycle(), mid_cycle());
        cleared.clear();
        soup.randomize(0.3, &mut rng);
        blob.randomize_blob(0.3, &mut rng);
        for universe in [cleared, soup, blob] {
            assert_eq!((universe.vacuum(), universe.generation), (0, 0));
        }
    }

    /// The vacuum of `universe` as cells: the tile [`Universe::vacuum`] describes, repeated.
    fn vacuum_cells(universe: &Universe) -> Vec<u8> {
        let offset = universe.partition_offset();
        (0..universe.height)
            .flat_map(|y| (0..universe.width).map(move |x| (x, y)))
            .map(|(x, y)| (universe.vacuum() >> (((x + offset) & 1) + 2 * ((y + offset) & 1))) & 1)
            .collect()
    }

    #[test]
    fn stored_cells_are_the_true_cells_but_for_the_vacuum() {
        // The automaton proper, stepped by the plain definition, against the stored difference.
        let mut rng = Rng::new(17);
        let mut rules: Vec<BlockRule> = PRESETS.iter().map(|preset| preset.rule()).collect();
        rules.extend((0..40).map(|_| BlockRule::random(|| rng.next_u64())));
        for rule in rules {
            let mut universe = soup(rule.clone(), 23);
            let mut truth = universe.cells().to_vec();
            let (width, height) = (universe.width, universe.height);
            let check = |universe: &Universe, truth: &[u8], when: &str| {
                let vacuum = vacuum_cells(universe);
                let stored: Vec<u8> = universe.cells().iter().zip(&vacuum).map(|(c, v)| c ^ v).collect();
                assert_eq!(stored, truth, "{rule}, {when} generation {}", universe.generation);
            };
            for _ in 0..20 {
                let offset = universe.partition_offset();
                truth = reference_step(&truth, width, height, rule.table_for(true), offset);
                universe.step(true);
                check(&universe, &truth, "forwards to");
            }
            for _ in 0..27 {
                universe.step(false);
                let offset = universe.partition_offset();
                truth = reference_step(&truth, width, height, rule.table_for(false), offset);
                check(&universe, &truth, "back to");
            }
        }
    }

    /// The lightest ship of Single Rotation, heading right, two blocks from the edge.
    fn ship_near_the_edge() -> Universe {
        let mut universe = Universe::new(32, 32, rule("single-rotation"));
        for (x, y) in [(26, 13), (27, 13), (26, 15), (27, 15)] {
            universe.set(x, y, true);
        }
        universe
    }

    #[test]
    fn the_open_border_takes_small_patterns_whole() {
        let mut universe = ship_near_the_edge();
        universe.open_border = true;
        let mut steps = 0;
        while universe.population() == 4 {
            universe.step(true);
            steps += 1;
            assert!(steps < 100, "the ship never reached the edge");
        }
        assert_eq!(universe.population(), 0, "it left in one piece");
        assert!(!universe.has_departures(), "nobody asked for it to be kept");
    }

    #[test]
    fn what_is_caught_is_kept_whatever_the_border() {
        for open_border in [false, true] {
            let mut universe = ship_near_the_edge();
            universe.open_border = open_border;
            universe.catching = true;
            universe.step_by(100);
            assert_eq!(universe.population(), 0);
            let departures = universe.take_departures();
            assert_eq!(departures.len(), 1);
            assert_eq!(departures[0].cells.len(), 4);
            assert_eq!(departures[0].phase, 0);
            assert!(universe.take_departures().is_empty(), "they are handed over once");
        }
    }

    #[test]
    fn both_lines_of_the_edge_are_watched() {
        let mut universe = Universe::new(16, 16, BlockRule::identity());
        universe.open_border = true;
        for (x, y) in [(5, 0), (0, 9), (5, 9)] {
            universe.set(x, y, true);
        }
        universe.step(true);
        assert_eq!(universe.population(), 1, "only the cell away from the edge is left");
        assert!(universe.get(5, 9));
    }

    #[test]
    fn a_pattern_across_the_edge_keeps_its_shape() {
        let mut universe = Universe::new(16, 16, BlockRule::identity());
        universe.catching = true;
        for (x, y) in [(15, 6), (0, 6), (14, 9)] {
            universe.set(x, y, true);
        }
        universe.step(true);
        assert_eq!(universe.population(), 0);
        let cells = &universe.take_departures()[0].cells;
        // Found from (0, 6); after one step the blocks start on odd coordinates.
        assert_eq!(cells, &[(1, 1), (0, 1), (-1, 4)]);
    }

    #[test]
    fn debris_is_worn_down_by_an_open_border_and_passes_a_closed_one() {
        // Six cells by six: too many to be a pattern.
        const { assert!(PATTERN_CELLS <= 36) };
        let debris = || {
            let mut universe = Universe::new(32, 32, BlockRule::identity());
            for y in 10..16 {
                for x in 0..6 {
                    universe.set(x, y, true);
                }
            }
            universe
        };
        let (mut open, mut closed) = (debris(), debris());
        open.open_border = true;
        closed.catching = true;
        open.step(true);
        closed.step(true);
        assert_eq!(open.population(), 30, "one column of six is gone");
        assert_eq!(closed.cells(), debris().cells());
        assert!(!open.has_departures() && !closed.has_departures());
    }

    #[test]
    fn resizing_keeps_the_pattern_and_its_place_among_the_blocks() {
        let mut universe = Universe::new(32, 32, rule("single-rotation"));
        for (x, y) in [(12, 13), (13, 13), (12, 15), (13, 15)] {
            universe.set(x, y, true);
        }
        let mut twin = universe.clone();
        // With room added all around and taken away again, the ship flies as it would have.
        universe.resize(64, 48);
        assert_eq!((universe.width, universe.height, universe.population()), (64, 48, 4));
        assert!(universe.get(12 + 16, 13 + 8));
        universe.step_by(12);
        twin.step_by(12);
        universe.resize(32, 32);
        assert_eq!(universe.cells(), twin.cells());
        assert_eq!(universe.generation, 12);
        // What no longer fits is cut off: the ship is now at (14, 13), two rows of two cells.
        universe.resize(4, 4);
        assert_eq!(universe.population(), 2);
        universe.step_by(3);
        assert_eq!(universe.population(), 2, "and the grid steps as before");
    }

    /// The definition of a step, written for clarity: every cell looks up its block.
    fn reference_step(cells: &[u8], width: usize, height: usize, table: &[u8; 16], offset: usize) -> Vec<u8> {
        let at = |x: usize, y: usize| cells[(y % height) * width + x % width];
        let mut next = vec![0; cells.len()];
        for by in (offset..offset + height).step_by(2) {
            for bx in (offset..offset + width).step_by(2) {
                let block = at(bx, by) | (at(bx + 1, by) << 1) | (at(bx, by + 1) << 2) | (at(bx + 1, by + 1) << 3);
                let out = table[block as usize];
                for (bit, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                    next[((by + dy) % height) * width + (bx + dx) % width] = (out >> bit) & 1;
                }
            }
        }
        next
    }

    #[test]
    fn the_kernel_matches_the_definition() {
        // Widths around the eight-cell chunks of the kernel, the smallest grids, tall and wide.
        let sizes = [
            (2, 2), (2, 6), (6, 2), (4, 4), (8, 4), (10, 6), (16, 2), (18, 8), (26, 6), (34, 4),
            (62, 8), (64, 6), (66, 10), (130, 12),
        ];
        let mut rng = Rng::new(5);
        let mut rules: Vec<BlockRule> = PRESETS.iter().map(|preset| preset.rule()).collect();
        rules.extend((0..6).map(|_| BlockRule::random(|| rng.next_u64())));
        for (width, height) in sizes {
            for rule in &rules {
                for forward in [true, false] {
                    let table = rule.table_for(forward);
                    let kernel = Kernel::new(table);
                    let mut cells = vec![0u8; width * height];
                    cells.iter_mut().for_each(|cell| *cell = (rng.next_f32() < 0.4) as u8);
                    for offset in [0, 1] {
                        let expected = reference_step(&cells, width, height, table, offset);
                        for parallel in [false, true] {
                            let mut next = vec![9u8; cells.len()];
                            step_blocks(&cells, &mut next, width, &kernel, offset, parallel);
                            assert_eq!(next, expected, "{width}×{height}, offset {offset}, {rule}");
                        }
                        cells = expected;
                    }
                }
            }
        }
    }

    /// Not a test, a stopwatch: `cargo test --release -- --ignored --nocapture stopwatch`.
    #[test]
    #[ignore]
    fn stopwatch() {
        for size in [256, 1024, 4096] {
            let mut universe = Universe::new(size, size, rule("critters"));
            universe.randomize(0.35, &mut Rng::new(3));
            let steps = ((1 << 28) / (size * size) as i64).max(64);
            universe.step_by(2);
            let started = Instant::now();
            universe.step_by(steps);
            let per_step = started.elapsed().as_secs_f64() / steps as f64;
            println!(
                "{size}×{size}: {:.1} µs per generation, {:.3} ns per cell",
                1e6 * per_step,
                1e9 * per_step / (size * size) as f64,
            );
        }
    }

    #[test]
    fn population_counts_every_cell() {
        let mut universe = Universe::new(600, 400, rule("single-rotation"));
        assert_eq!(universe.population(), 0);
        universe.cells.fill(1);
        assert_eq!(universe.population(), 240_000);
    }

    #[test]
    fn a_lone_cell_rotates_within_its_block() {
        // Generation 0 uses the origin-aligned partition: (1, 0) is the top-right cell of the
        // block at (0, 0), so one clockwise rotation moves it to the bottom-right corner (1, 1).
        let mut universe = Universe::new(4, 4, rule("single-rotation"));
        universe.set(1, 0, true);
        universe.step(true);
        assert!(universe.get(1, 1));
        assert_eq!(universe.population(), 1);
        // Generation 1 uses the shifted partition: (1, 1) is now the top-left cell of the block
        // at (1, 1), so it moves to the top-right corner (2, 1).
        universe.step(true);
        assert!(universe.get(2, 1));
        universe.step_by(-2);
        assert!(universe.get(1, 0));
    }
}

/// Patterns published with the Single Rotation rule (dmishin's blog and the simulator links in
/// it), placed where those links place them. Together they pin down everything that depends on
/// convention: the sense of rotation, the bit layout of the table and which partition comes
/// first. A mirrored rule or a shifted phase fails them.
#[cfg(test)]
mod published_patterns {
    use super::*;
    use std::collections::BTreeSet;

    type Cells = BTreeSet<(i32, i32)>;

    fn rle(pattern: &str) -> Vec<(i32, i32)> {
        crate::pattern::from_rle(pattern).unwrap()
    }

    fn place(cells: &[(i32, i32)], origin: (i32, i32), size: usize) -> Universe {
        let mut universe = Universe::new(size, size, "single-rotation".parse().unwrap());
        for (x, y) in cells {
            universe.set((origin.0 + x) as usize, (origin.1 + y) as usize, true);
        }
        universe
    }

    fn live(universe: &Universe) -> Cells {
        (0..universe.height)
            .flat_map(|y| (0..universe.width).map(move |x| (x, y)))
            .filter(|&(x, y)| universe.get(x, y))
            .map(|(x, y)| (x as i32, y as i32))
            .collect()
    }

    fn shifted(cells: &Cells, by: (i32, i32)) -> Cells {
        cells.iter().map(|&(x, y)| (x + by.0, y + by.1)).collect()
    }

    /// After `period` steps the pattern must be back, displaced by `by`.
    fn assert_travels(pattern: &str, origin: (i32, i32), period: i64, by: (i32, i32)) {
        let mut universe = place(&rle(pattern), origin, 128);
        let start = live(&universe);
        universe.step_by(period);
        assert_eq!(live(&universe), shifted(&start, by), "{pattern} after {period} steps");
    }

    #[test]
    fn lightest_orthogonal_spaceships_move_at_c_over_6() {
        assert_travels("bo2$b2o$bo", (64, 64), 12, (2, 0));
        // As placed by the blog's "lightest spaceship and single cell collision" link.
        assert_travels("2o2$2o", (10, 25), 12, (2, 0));
    }

    #[test]
    fn conways_glider_shape_glides_with_period_15() {
        // An odd period with an odd shift: the same shape on the other partition.
        assert_travels("3o$o$bo", (28, 28), 15, (1, 1));
    }

    #[test]
    fn slow_diagonal_spaceship_moves_at_c_over_184() {
        assert_travels("o$o2$o$o", (64, 64), 368, (2, 2));
    }

    #[test]
    fn the_mirror_image_is_a_different_pattern() {
        let cells = rle("bo2$b2o$bo");
        let width = cells.iter().map(|c| c.0).max().unwrap();
        let mirrored: Vec<_> = cells.iter().map(|&(x, y)| (width - x, y)).collect();
        let mut universe = place(&mirrored, (64, 64), 128);
        let start = live(&universe);
        universe.step_by(12);
        let end = live(&universe);
        let reappears = (-12..=12)
            .flat_map(|dx| (-12..=12).map(move |dy| (dx, dy)))
            .any(|by| shifted(&start, by) == end);
        assert!(!reappears, "the rule has no mirror symmetry");
    }

    #[test]
    fn ship_reaches_and_hits_the_lone_cell() {
        // The blog's demo, as linked: rle_x0=10, rle_y0=25 on a 64×64 torus. The ship flies
        // right towards a cell 18 columns away and the two collide.
        let mut universe = place(&rle("2o18bo2$2o"), (10, 25), 64);
        universe.step_by(96);
        let expected: Cells = [(26, 25), (27, 25), (26, 27), (27, 27), (30, 25)].into();
        assert_eq!(live(&universe), expected);
        universe.step_by(24);
        let ship: Cells = [(28, 25), (29, 25), (28, 27), (29, 27)].into();
        assert!(!ship.is_subset(&live(&universe)), "the ship should have been disturbed");
        assert_eq!(universe.population(), 5);
    }

    #[test]
    fn block_is_a_still_life_only_when_it_straddles_the_partitions() {
        // On the mixed alignment every block of either partition sees two of its cells.
        for origin in [(10, 11), (11, 10)] {
            let mut universe = place(&rle("2o$2o"), origin, 32);
            let start = live(&universe);
            for _ in 0..4 {
                universe.step(true);
                assert_eq!(live(&universe), start);
            }
        }
        // Aligned with a partition, it is four lone cells half of the time.
        let mut universe = place(&rle("2o$2o"), (10, 10), 32);
        let start = live(&universe);
        universe.step_by(2);
        assert_ne!(live(&universe), start);
        universe.step_by(2);
        assert_eq!(live(&universe), start);
    }

    #[test]
    fn a_lone_cell_orbits_counterclockwise_with_period_4() {
        // Each step is a clockwise quarter turn inside the current block, but the blocks
        // alternate, and the net path runs the other way round, whatever the alignment.
        for (ax, ay) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let mut universe = place(&[(0, 0)], (16 + ax, 16 + ay), 32);
            let mut path = vec![(16 + ax, 16 + ay)];
            for _ in 0..4 {
                universe.step(true);
                path.push(*live(&universe).iter().next().unwrap());
            }
            assert_eq!(path.first(), path.last());
            assert_eq!(path.iter().collect::<BTreeSet<_>>().len(), 4);
            // Shoelace sum: with y pointing down, positive means clockwise on screen.
            let turn: i32 = path.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum();
            assert!(turn < 0, "expected a counterclockwise orbit, got {path:?}");
        }
    }
}
