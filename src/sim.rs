//! The universe: a toroidal grid of cells plus the machinery to step it forwards and backwards.

use std::time::Duration;

use bevy::{platform::time::Instant, prelude::*};
use rayon::prelude::*;

use crate::rules::{BlockRule, Vacuum};

/// The order of a frame in `Update`: input changes the world and the transport, the simulation
/// steps, then everything that shows the result catches up.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSystems {
    Input,
    Step,
    Present,
}

/// SplitMix64: tiny, fast and deterministic, which is all a random soup needs.
#[derive(Resource, Clone, Debug)]
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

/// The grid of cells. Cells are stored row-major as `0`/`1` bytes; the grid wraps around.
#[derive(Resource, Clone, Debug)]
pub struct Universe {
    pub width: usize,
    pub height: usize,
    cells: Vec<u8>,
    scratch: Vec<u8>,
    /// How many steps we are from the initial condition; the partition offset follows its parity.
    pub generation: i64,
    rule: BlockRule,
    forward: Kernel,
    backward: Kernel,
    flipped: bool,
}

impl Universe {
    pub fn new(width: usize, height: usize, rule: BlockRule) -> Self {
        assert!(
            width >= 2 && height >= 2 && width.is_multiple_of(2) && height.is_multiple_of(2),
            "a Margolus grid needs even dimensions, got {width}×{height}"
        );
        let n = width * height;
        Self {
            width,
            height,
            cells: vec![0; n],
            scratch: vec![0; n],
            generation: 0,
            forward: Kernel::new(rule.table_for(true)),
            backward: Kernel::new(rule.table_for(false)),
            rule,
            flipped: false,
        }
    }

    pub fn rule(&self) -> &BlockRule {
        &self.rule
    }

    /// Switches rule in place; the cells and the generation counter are kept. Leaving a
    /// vacuum-flipping rule in its all-alive phase complements the cells: the new rule starts
    /// from a dead vacuum and the picture carries over.
    pub fn set_rule(&mut self, rule: BlockRule) {
        if self.flipped && rule.vacuum() != Vacuum::Flips {
            self.cells.iter_mut().for_each(|cell| *cell ^= 1);
            self.flipped = false;
        }
        self.forward = Kernel::new(rule.table_for(true));
        self.backward = Kernel::new(rule.table_for(false));
        self.rule = rule;
    }

    /// True while a vacuum-flipping rule (Critters, Tron) has the world in its all-alive phase:
    /// an odd number of steps was taken under such a rule since the world was last reset. The
    /// picture is then easier to read complemented, see [`Settings::shows_complement`].
    pub fn is_flipped(&self) -> bool {
        self.flipped
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

    /// A fresh pattern is generation 0 with a dead vacuum.
    fn reset_clock(&mut self) {
        self.generation = 0;
        self.flipped = false;
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
        if forward {
            let offset = Self::offset_at(self.generation);
            self.apply(true, offset);
            self.generation += 1;
        } else {
            self.generation -= 1;
            let offset = Self::offset_at(self.generation);
            self.apply(false, offset);
        }
    }

    /// Positive `steps` go forwards in time, negative ones backwards.
    pub fn step_by(&mut self, steps: i64) {
        for _ in 0..steps.unsigned_abs() {
            self.step(steps > 0);
        }
    }

    fn apply(&mut self, forward: bool, offset: usize) {
        let kernel = if forward { &self.forward } else { &self.backward };
        let parallel = self.cells.len() >= PARALLEL_CELLS;
        step_blocks(&self.cells, &mut self.scratch, self.width, kernel, offset, parallel);
        std::mem::swap(&mut self.cells, &mut self.scratch);
        if self.rule.vacuum() == Vacuum::Flips {
            self.flipped = !self.flipped;
        }
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

/// Transport state. `speed` is frames per second; each frame advances `stride` generations, so
/// "render every Nth step" is simply `stride = N`.
#[derive(Resource, Clone, Debug)]
pub struct Playback {
    pub playing: bool,
    pub reverse: bool,
    pub speed: f32,
    pub stride: u32,
    /// How long one update may spend stepping. Work beyond it is dropped, not owed.
    pub budget: Duration,
}

impl Playback {
    pub const MIN_SPEED: f32 = 0.5;
    pub const MAX_SPEED: f32 = 240.0;
    pub const MAX_STRIDE: u32 = 512;

    /// `+1` forwards, `-1` backwards.
    pub fn direction(&self) -> i64 {
        if self.reverse { -1 } else { 1 }
    }
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            playing: false,
            reverse: false,
            speed: 30.0,
            stride: 1,
            budget: Duration::from_millis(12),
        }
    }
}

/// How fast the simulation really runs when it cannot keep up with [`Playback`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Pace {
    /// Generations per second, measured; `None` while the requested rate is met.
    pub achieved: Option<f32>,
}

/// Everything that is not transport: how the world is drawn and seeded.
#[derive(Resource, Clone, Debug)]
pub struct Settings {
    /// Draw the complement while a vacuum-flipping rule has the world in its all-alive phase,
    /// so empty space stays dark instead of flickering.
    pub hide_vacuum: bool,
    /// Live-cell probability of a random soup.
    pub density: f32,
    /// Outline the cells (when zoomed in far enough to see them).
    pub show_grid: bool,
    /// Outline the current partition: the 2×2 blocks a forward step rewrites next.
    pub show_blocks: bool,
}

impl Settings {
    pub const MIN_DENSITY: f32 = 0.0001;
    pub const MAX_DENSITY: f32 = 0.9;

    /// Is the picture the complement of the cells right now? Whatever shows or edits cells on
    /// screen (the shader, painting, the population figure) goes through this.
    pub fn shows_complement(&self, universe: &Universe) -> bool {
        self.hide_vacuum && universe.is_flipped()
    }
}

/// Run condition: has the rule changed since this condition last looked? The universe as a
/// whole counts as changed on every step, so its own change flag cannot tell.
pub fn rule_changed(universe: Res<Universe>, mut seen: Local<Option<BlockRule>>) -> bool {
    let changed = seen.as_ref() != Some(universe.rule());
    if changed {
        *seen = Some(universe.rule().clone());
    }
    changed
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Playback>()
            .init_resource::<Pace>()
            .configure_sets(
                Update,
                (SimSystems::Input, SimSystems::Step, SimSystems::Present).chain(),
            )
            .add_systems(Update, advance.in_set(SimSystems::Step));
    }
}

/// The rate actually achieved is measured over windows of this length.
const PACE_WINDOW: Duration = Duration::from_millis(500);

#[derive(Default)]
struct PaceMeter {
    since: Option<Instant>,
    generations: u64,
    lagging: bool,
}

/// Runs the simulation while playing: one frame of `stride` generations every `1 / speed`
/// seconds. Frames are never split, so the picture always shows a multiple of the stride.
/// When the machine cannot keep up, the frames still owed after `budget` are dropped: the
/// simulation slows down instead of freezing the window.
fn advance(
    time: Res<Time>,
    playback: Res<Playback>,
    mut universe: ResMut<Universe>,
    mut pace: ResMut<Pace>,
    mut owed: Local<f64>,
    mut meter: Local<PaceMeter>,
) {
    if !playback.playing {
        *owed = 0.0;
        *meter = PaceMeter::default();
        pace.set_if_neq(Pace::default());
        return;
    }

    *owed += time.delta_secs_f64() * playback.speed as f64;
    let due = owed.floor() as u32;
    let started = Instant::now();
    let mut done = 0;
    while done < due {
        universe.step_by(playback.stride as i64 * playback.direction());
        done += 1;
        if started.elapsed() >= playback.budget {
            break;
        }
    }
    let lagging = done < due;
    *owed = if lagging { 0.0 } else { *owed - due as f64 };

    meter.generations += done as u64 * playback.stride as u64;
    meter.lagging |= lagging;
    let elapsed = meter.since.get_or_insert(started).elapsed();
    if elapsed >= PACE_WINDOW {
        let rate = meter.generations as f32 / elapsed.as_secs_f32();
        pace.set_if_neq(Pace {
            achieved: meter.lagging.then_some(rate),
        });
        *meter = PaceMeter::default();
    }
}

#[cfg(test)]
mod tests {
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
    fn critters_conserves_population_over_even_steps() {
        // Every step maps a block's live count c to 4 - c (or keeps 2), so after one step the
        // population is `cells - n` and after two it is `n` again.
        let mut universe = soup(rule("critters"), 11);
        let population = universe.population();
        let cells = universe.width * universe.height;
        universe.step(true);
        assert_eq!(universe.population(), cells - population);
        universe.step_by(99);
        assert_eq!(universe.population(), population);
    }

    #[test]
    fn critters_complements_the_vacuum_each_step() {
        let mut universe = Universe::new(8, 8, rule("critters"));
        universe.step(true);
        assert_eq!(universe.population(), 64);
        assert!(universe.is_flipped());
        universe.step(true);
        assert_eq!(universe.population(), 0);
        assert!(!universe.is_flipped());
        universe.step(false);
        assert!(universe.is_flipped(), "stepping back flips the vacuum too");
    }

    #[test]
    fn the_picture_survives_a_rule_switch() {
        // Three steps of Critters leave the world in its all-alive phase. What the user sees,
        // with the vacuum hidden, is the complement; that is what the next rule must get.
        let mut universe = soup(rule("critters"), 21);
        universe.step_by(3);
        let picture: Vec<u8> = universe.cells().iter().map(|cell| cell ^ 1).collect();

        let mut to_tron = universe.clone();
        to_tron.set_rule(rule("tron"));
        assert!(to_tron.is_flipped(), "Tron flips the vacuum as well: nothing to undo");
        assert_ne!(to_tron.cells(), picture);

        universe.set_rule(rule("single-rotation"));
        assert!(!universe.is_flipped());
        assert_eq!(universe.cells(), picture);
        assert_eq!(universe.generation, 3, "the clock is not touched");
    }

    #[test]
    fn a_new_pattern_starts_with_a_dead_vacuum() {
        let mut rng = Rng::new(1);
        let flipped = || {
            let mut universe = soup(rule("critters"), 4);
            universe.step(true);
            universe
        };
        let (mut cleared, mut soup, mut blob) = (flipped(), flipped(), flipped());
        cleared.clear();
        soup.randomize(0.3, &mut rng);
        blob.randomize_blob(0.3, &mut rng);
        for universe in [cleared, soup, blob] {
            assert!(!universe.is_flipped());
            assert_eq!(universe.generation, 0);
        }
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

/// The pacing system, driven by a headless app whose clock advances a quarter second per update.
#[cfg(test)]
mod pacing {
    use super::*;
    use bevy::time::TimeUpdateStrategy;

    const TICK: Duration = Duration::from_millis(250);

    fn app(playback: Playback) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(TICK))
            .insert_resource(Universe::new(8, 8, "single-rotation".parse().unwrap()))
            .insert_resource(playback);
        // The first update only starts the clock.
        app.update();
        app
    }

    fn generation(app: &App) -> i64 {
        app.world().resource::<Universe>().generation
    }

    #[test]
    fn a_frame_is_a_whole_stride() {
        // 8 frames per second for a quarter second: two frames of three generations.
        let mut app = app(Playback { playing: true, speed: 8.0, stride: 3, ..default() });
        app.update();
        assert_eq!(generation(&app), 6);
        app.update();
        assert_eq!(generation(&app), 12);
        assert_eq!(app.world().resource::<Pace>().achieved, None);
    }

    #[test]
    fn slow_speeds_wait_for_their_frame() {
        let mut app = app(Playback { playing: true, speed: 1.0, stride: 5, ..default() });
        for _ in 0..3 {
            app.update();
            assert_eq!(generation(&app), 0);
        }
        app.update();
        assert_eq!(generation(&app), 5);
    }

    #[test]
    fn reverse_and_pause() {
        let mut app = app(Playback { playing: true, reverse: true, speed: 4.0, ..default() });
        app.update();
        assert_eq!(generation(&app), -1);
        app.world_mut().resource_mut::<Playback>().playing = false;
        app.update();
        assert_eq!(generation(&app), -1);
    }

    #[test]
    fn work_over_budget_is_dropped_not_owed() {
        // 60 frames are due per update, but with no budget only the first one runs; the rest
        // must not pile up, and the stride stays whole.
        let playback = Playback {
            playing: true,
            speed: 240.0,
            stride: 7,
            budget: Duration::ZERO,
            ..default()
        };
        let mut app = app(playback);
        for update in 1..=4 {
            app.update();
            assert_eq!(generation(&app), 7 * update);
        }
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

    /// Cells of an RLE pattern: `b` dead, `o` alive, `$` end of row, digits repeat.
    fn rle(pattern: &str) -> Vec<(i32, i32)> {
        let (mut x, mut y, mut count) = (0, 0, 0);
        let mut cells = Vec::new();
        for c in pattern.chars() {
            match c {
                '0'..='9' => count = count * 10 + c.to_digit(10).unwrap() as i32,
                'b' => {
                    x += count.max(1);
                    count = 0;
                }
                'o' => {
                    for _ in 0..count.max(1) {
                        cells.push((x, y));
                        x += 1;
                    }
                    count = 0;
                }
                '$' => {
                    y += count.max(1);
                    x = 0;
                    count = 0;
                }
                _ => panic!("bad RLE character {c:?}"),
            }
        }
        cells
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
