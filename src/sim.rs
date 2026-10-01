//! The universe: a toroidal grid of cells plus the machinery to step it forwards and backwards.

use bevy::prelude::*;
use rayon::prelude::*;

use crate::rules::{BlockRule, RuleKind};

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
    pub cells: Vec<u8>,
    scratch: Vec<u8>,
    /// How many steps we are from the initial condition; the partition offset follows its parity.
    pub generation: i64,
    pub rule: BlockRule,
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
            rule,
        }
    }

    pub fn kind(&self) -> RuleKind {
        self.rule.kind
    }

    /// Switches rule in place; the cells and the generation counter are kept.
    pub fn set_rule(&mut self, kind: RuleKind) {
        if self.rule.kind != kind {
            self.rule = kind.rule();
        }
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        self.cells[y * self.width + x] != 0
    }

    pub fn set(&mut self, x: usize, y: usize, alive: bool) {
        self.cells[y * self.width + x] = alive as u8;
    }

    pub fn clear(&mut self) {
        self.cells.fill(0);
        self.generation = 0;
    }

    pub fn randomize(&mut self, density: f32, rng: &mut Rng) {
        for cell in &mut self.cells {
            *cell = (rng.next_f32() < density) as u8;
        }
        self.generation = 0;
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
        self.generation = 0;
    }

    pub fn population(&self) -> usize {
        self.cells.iter().map(|&c| c as usize).sum()
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
        for _ in 0..steps.abs() {
            self.step(steps > 0);
        }
    }

    fn apply(&mut self, forward: bool, offset: usize) {
        step_blocks(
            &self.cells,
            &mut self.scratch,
            self.width,
            self.height,
            self.rule.table_for(forward),
            offset,
        );
        std::mem::swap(&mut self.cells, &mut self.scratch);
    }
}

/// One Margolus step: every 2×2 block whose top-left corner sits at
/// `(offset + 2i, offset + 2j)` (wrapping) is replaced by `table[block]`.
///
/// Output rows are independent given the read-only input, so rayon splits the work by row. Each
/// row only recomputes the blocks it belongs to, so a block is evaluated twice (once per row).
pub fn step_blocks(
    cells: &[u8],
    next: &mut [u8],
    width: usize,
    height: usize,
    table: &[u8; 16],
    offset: usize,
) {
    debug_assert!(width.is_multiple_of(2) && height.is_multiple_of(2));
    debug_assert_eq!(cells.len(), width * height);
    debug_assert_eq!(next.len(), width * height);

    next.par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            // Is this row the bottom half of its block? (`height` is even, so adding it keeps parity.)
            let bottom = (y + height - offset) % 2 == 1;
            let partner = if bottom {
                (y + height - 1) % height
            } else {
                (y + 1) % height
            };
            let (top_y, bottom_y) = if bottom { (partner, y) } else { (y, partner) };
            let top = &cells[top_y * width..][..width];
            let bot = &cells[bottom_y * width..][..width];
            let shift = if bottom { 2 } else { 0 };

            for k in 0..width / 2 {
                let x0 = (offset + 2 * k) % width;
                let x1 = (x0 + 1) % width;
                let block = top[x0] | (top[x1] << 1) | (bot[x0] << 2) | (bot[x1] << 3);
                let out = table[block as usize];
                row[x0] = (out >> shift) & 1;
                row[x1] = (out >> (shift + 1)) & 1;
            }
        });
}

/// Transport state. `speed` is rendered frames per second; each frame advances `stride`
/// generations, so "render every Nth step" is simply `stride = N`.
#[derive(Resource, Clone, Debug)]
pub struct Playback {
    pub playing: bool,
    pub reverse: bool,
    pub speed: f32,
    pub stride: u32,
    accumulator: f64,
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
            accumulator: 0.0,
        }
    }
}

/// Everything that is not transport: how the world is drawn and seeded.
#[derive(Resource, Clone, Debug)]
pub struct Settings {
    /// Draw the complement on odd generations for rules whose vacuum flips, so empty space
    /// stays dark instead of flickering.
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
}

/// Hard cap on the work done per frame when the display can't keep up.
const MAX_STEPS_PER_UPDATE: i64 = 1 << 14;

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, advance);
    }
}

/// Runs the simulation when playing. The accumulator is updated without triggering change
/// detection so observers of `Playback` only fire for real control changes.
pub fn advance(time: Res<Time>, mut playback: ResMut<Playback>, mut universe: ResMut<Universe>) {
    let playback = playback.bypass_change_detection();
    if !playback.playing {
        playback.accumulator = 0.0;
        return;
    }
    playback.accumulator += time.delta_secs_f64() * playback.speed as f64;
    let frames = playback.accumulator.floor();
    if frames < 1.0 {
        return;
    }
    playback.accumulator -= frames;
    let steps = (frames as i64 * playback.stride as i64).min(MAX_STEPS_PER_UPDATE);
    universe.step_by(steps * playback.direction());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soup(kind: RuleKind, seed: u64) -> Universe {
        let mut rng = Rng::new(seed);
        let mut universe = Universe::new(32, 16, kind.rule());
        universe.randomize(0.35, &mut rng);
        universe
    }

    #[test]
    fn stepping_back_restores_the_past() {
        for kind in RuleKind::ALL {
            let start = soup(kind, 7);
            let mut universe = start.clone();
            universe.step_by(37);
            assert_ne!(universe.cells, start.cells, "{kind}: nothing happened");
            universe.step_by(-37);
            assert_eq!(universe.generation, 0);
            assert_eq!(universe.cells, start.cells, "{kind}: not reversible");
        }
    }

    #[test]
    fn single_rotation_conserves_population() {
        let mut universe = soup(RuleKind::SingleRotation, 3);
        let population = universe.population();
        universe.step_by(100);
        assert_eq!(universe.population(), population);
    }

    #[test]
    fn critters_conserves_population_over_even_steps() {
        // Every step maps a block's live count c to 4 - c (or keeps 2), so after one step the
        // population is `cells - n` and after two it is `n` again.
        let mut universe = soup(RuleKind::Critters, 11);
        let population = universe.population();
        let cells = universe.width * universe.height;
        universe.step(true);
        assert_eq!(universe.population(), cells - population);
        universe.step_by(99);
        assert_eq!(universe.population(), population);
    }

    #[test]
    fn critters_complements_the_vacuum_each_step() {
        let mut universe = Universe::new(8, 8, RuleKind::Critters.rule());
        universe.step(true);
        assert_eq!(universe.population(), 64);
        universe.step(true);
        assert_eq!(universe.population(), 0);
    }

    #[test]
    fn a_lone_cell_rotates_within_its_block() {
        // Generation 0 uses the origin-aligned partition: (1, 0) is the top-right cell of the
        // block at (0, 0), so one clockwise rotation moves it to the bottom-right corner (1, 1).
        let mut universe = Universe::new(4, 4, RuleKind::SingleRotation.rule());
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
        let mut universe = Universe::new(size, size, RuleKind::SingleRotation.rule());
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
