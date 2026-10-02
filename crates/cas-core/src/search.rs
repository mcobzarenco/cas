//! Looking for rules worth looking at.
//!
//! No one number says that a rule is interesting. What can be measured cheaply is what a rule
//! does not do: it does not freeze, it does not boil, and things in it hold together and move.
//! So a rule is put through a few independent trials ([`measure`]), and its [`Report`] says how
//! each went:
//!
//! * **Seeds.** Small random patterns are left alone on an unbounded plane. Each one comes back
//!   to its shape, in place or elsewhere, or grows without bound, or flies apart.
//! * **Spaceships.** What comes back elsewhere, slower than light, is counted by kind; so is
//!   what flies out of a blob through an open border.
//! * **Damage.** One cell of a random soup is flipped. A hundred generations on, how much of
//!   what the flip could have reached is different?
//! * **Evaporation.** How much of that blob is left in the end.
//!
//! Most rules fail the first trial: nearly every seed explodes. Of the rest, the ones with
//! several kinds of spaceship, many periods and little damage are the ones to look at.

use std::collections::{BTreeSet, HashSet};

use rayon::prelude::*;

use crate::{
    census::Census,
    pattern::{Analyser, Cell, Fate, Motion},
    rules::{BlockRule, complement, popcount},
    universe::{Rng, Universe},
};

/// A seed with more cells than this is growing, and one wider than this has flown apart.
const SEED_CELLS: usize = 120;
const SEED_EXTENT: i32 = 160;
/// The grid of the soup and of the blob.
const GRID: usize = 256;
const SOUP_DENSITY: f32 = 0.15;
const BLOB_DENSITY: f32 = 0.3;
const DAMAGE_GENERATIONS: i64 = 100;
/// No more catches than this are identified: a boiling rule would never be done.
const CATCHES: u64 = 30_000;

/// How hard to look.
#[derive(Clone, Debug)]
pub struct Effort {
    /// Small random patterns left alone on the unbounded plane.
    pub seeds: usize,
    /// For how many generations each is followed before it counts as undecided.
    pub generations: u32,
    /// For how many generations the blob is left to evaporate. With 0 that trial is skipped.
    pub evaporation: i64,
}

impl Default for Effort {
    fn default() -> Self {
        Self {
            seeds: 400,
            generations: 3000,
            evaporation: 8000,
        }
    }
}

/// How a rule did in its trials.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// The shares of the seeds that came back to their shape in place and elsewhere, that
    /// flew apart, that grew without bound, and that did none of it in time.
    pub oscillating: f32,
    pub travelling: f32,
    pub scattering: f32,
    pub growing: f32,
    pub undecided: f32,
    /// Kinds of spaceship slower than light, among the seeds and from the blob.
    pub spaceships: usize,
    /// How many different periods the oscillating seeds had, and the longest.
    pub periods: usize,
    pub longest_period: u32,
    /// The share that differs of the cells a flipped cell could have reached.
    pub damage: f32,
    /// What is left of the blob, as a share of its cells; more than 1 if it grew.
    pub remaining: f32,
    /// How many spaceships left the blob, and how many small patterns that were none.
    pub caught: u64,
    pub others: u64,
}

/// What kind of world a rule makes, as far as its trials tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Character {
    /// Nearly every seed grows without bound.
    Explosive,
    /// Some seeds grow without bound.
    Growing,
    /// Something slower than light travels, and nothing much explodes.
    Spaceships,
    /// Seeds fly apart, and all that travels does so at the speed of light.
    Gas,
    /// Every seed stays where it is, and a change does not spread either.
    Frozen,
    /// Every seed stays where it is, yet a change spreads.
    Confined,
    Other,
}

impl Report {
    pub fn character(&self) -> Character {
        let stays = self.oscillating + self.undecided >= 0.995;
        if self.growing >= 0.9 {
            Character::Explosive
        } else if self.growing >= 0.1 {
            Character::Growing
        } else if self.spaceships > 0 {
            Character::Spaceships
        } else if stays && self.damage < 0.0005 {
            Character::Frozen
        } else if stays {
            Character::Confined
        } else if self.scattering > 0.5 {
            Character::Gas
        } else {
            Character::Other
        }
    }
}

/// Puts a rule through its trials. The same rule always gets the same report.
pub fn measure(rule: &BlockRule, effort: &Effort) -> Report {
    let mut analyser = Analyser::new(rule);
    analyser.max_generations = effort.generations;
    analyser.max_cells = SEED_CELLS;
    analyser.max_extent = SEED_EXTENT;

    let mut spaceships: HashSet<Vec<Cell>> = HashSet::new();
    let mut periods = BTreeSet::new();
    let (mut oscillating, mut travelling, mut scattering, mut growing, mut undecided) = (0, 0, 0, 0, 0);
    for seed in seeds(effort.seeds) {
        match analyser.fate(&seed, 0) {
            Fate::Returns { period, displacement: (0, 0) } => {
                oscillating += 1;
                periods.insert(period);
            }
            Fate::Returns { .. } => {
                travelling += 1;
                // Several ships side by side are no kind of their own.
                if let Some(motion) = analyser.analyse(&seed, 0)
                    && slower_than_light(&motion)
                    && analyser.parts(&seed, 0, motion.period).len() == 1
                {
                    spaceships.insert(motion.canonical);
                }
            }
            Fate::Scatters => scattering += 1,
            Fate::Grows => growing += 1,
            Fate::Undecided => undecided += 1,
        }
    }
    let share = |count: u32| count as f32 / effort.seeds.max(1) as f32;

    let (remaining, caught, others) = evaporate(rule, effort.evaporation, analyser, &mut spaceships);
    Report {
        oscillating: share(oscillating),
        travelling: share(travelling),
        scattering: share(scattering),
        growing: share(growing),
        undecided: share(undecided),
        spaceships: spaceships.len(),
        periods: periods.len(),
        longest_period: periods.last().copied().unwrap_or(0),
        damage: damage(rule),
        remaining,
        caught,
        others,
    }
}

/// Measures every rule, on as many threads as there are, and hands each report over as soon
/// as it is there.
pub fn survey(rules: &[BlockRule], effort: &Effort, done: impl Fn(&BlockRule, Report) + Sync) {
    rules.par_iter().for_each(|rule| done(rule, measure(rule, effort)));
}

/// The patterns every rule is tried on: one to six cells in a box five cells wide.
fn seeds(count: usize) -> Vec<Vec<Cell>> {
    let mut rng = Rng::new(9);
    let mut cell = || ((rng.next_u64() % 5) as i32, (rng.next_u64() % 5) as i32);
    (0..count)
        .map(|seed| {
            let mut cells: Vec<Cell> = (0..=seed % 6).map(|_| cell()).collect();
            cells.sort_unstable();
            cells.dedup();
            cells
        })
        .collect()
}

fn slower_than_light(motion: &Motion) -> bool {
    let (distance, period) = motion.speed();
    distance < period
}

/// Flips one cell in the middle of a soup: the share that differs, some generations on, of
/// the cells the flip could have reached.
fn damage(rule: &BlockRule) -> f32 {
    let mut soup = Universe::new(GRID, GRID, rule.clone());
    soup.randomize(SOUP_DENSITY, &mut Rng::new(5));
    let mut flipped = soup.clone();
    let middle = GRID / 2;
    flipped.set(middle, middle, !flipped.get(middle, middle));
    soup.step_by(DAMAGE_GENERATIONS);
    flipped.step_by(DAMAGE_GENERATIONS);
    let differing = soup.cells().iter().zip(flipped.cells()).filter(|(a, b)| a != b).count();
    let reach = 2 * DAMAGE_GENERATIONS + 1;
    differing as f32 / (reach * reach) as f32
}

/// Leaves a blob to evaporate through an open border. Returns what is left of it, and how
/// many spaceships and other small patterns left; the kinds of the slow ones join `spaceships`.
fn evaporate(
    rule: &BlockRule,
    generations: i64,
    analyser: Analyser,
    spaceships: &mut HashSet<Vec<Cell>>,
) -> (f32, u64, u64) {
    let mut universe = Universe::new(GRID, GRID, rule.clone());
    universe.randomize_blob(BLOB_DENSITY, &mut Rng::new(42));
    let cells = universe.population();
    universe.open_border = true;
    universe.catching = true;
    let mut census = Census::with(analyser);
    // Catches are looked at every so often, not after every generation.
    const STRIDE: i64 = 64;
    for _ in 0..generations / STRIDE {
        universe.step_by(STRIDE);
        for departure in universe.take_departures() {
            if census.ships() + census.others() < CATCHES {
                census.record(departure);
            }
        }
    }
    let slow = census.kinds().iter().filter(|kind| slower_than_light(&kind.motion));
    spaceships.extend(slow.map(|kind| kind.motion.canonical.clone()));
    let remaining = universe.population() as f32 / cells.max(1) as f32;
    (remaining, census.ships(), census.others())
}

/// Every reversible rule that looks the same after a quarter turn: Morita's 1536 ESPCAs.
pub fn rotation_symmetric() -> Vec<BlockRule> {
    let digits = |choices: &'static str| choices.chars();
    let mut rules = Vec::new();
    for u in digits("0f") {
        for v in digits("0123456789abcdef") {
            for w in digits("0123456789abcdef") {
                for x in digits("05af") {
                    for y in digits("0123456789abcdef") {
                        for z in digits("0f") {
                            let number: String = [u, v, w, x, y, z].iter().collect();
                            rules.extend(BlockRule::from_espca(&number));
                        }
                    }
                }
            }
        }
    }
    rules
}

/// Every rule that takes each block to one with as many cells, and every rule that takes each
/// block to one with as many dead cells as it had live ones, as Critters does. Under all of
/// them a pattern keeps its number of cells: 829 440 rules.
pub fn conserving() -> Vec<BlockRule> {
    let with = |cells: u32| -> Vec<u8> { (0..16).filter(|&block| popcount(block) == cells).collect() };
    let (ones, twos, threes) = (with(1), with(2), with(3));
    let mut rules = Vec::new();
    for one in permutations(&ones) {
        for two in permutations(&twos) {
            for three in permutations(&threes) {
                let mut keeping = [0u8; 16];
                keeping[15] = 15;
                let outcomes = one.iter().chain(&two).chain(&three);
                for (&block, &outcome) in ones.iter().chain(&twos).chain(&threes).zip(outcomes) {
                    keeping[block as usize] = outcome;
                }
                // The same after exchanging dead and alive: that trades the two counts.
                let trading = std::array::from_fn(|block| keeping[complement(block as u8) as usize]);
                rules.extend([keeping, trading].map(|table| BlockRule::new(table).expect("a permutation")));
            }
        }
    }
    rules
}

/// So many rules, each a random permutation of the sixteen blocks.
pub fn random(count: usize, seed: u64) -> Vec<BlockRule> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| BlockRule::random(|| rng.next_u64())).collect()
}

/// One rule for each set of rules that differ only in how one looks at them
/// ([`BlockRule::representative`]), in the order they first come up.
pub fn distinct(rules: impl IntoIterator<Item = BlockRule>) -> Vec<BlockRule> {
    let mut seen = HashSet::new();
    let representatives = rules.into_iter().map(|rule| rule.representative());
    representatives.filter(|rule| seen.insert(rule.clone())).collect()
}

/// Every order the items can be put in.
fn permutations(items: &[u8]) -> Vec<Vec<u8>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut orders = Vec::new();
    for (i, &first) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(i);
        for mut order in permutations(&rest) {
            order.insert(0, first);
            orders.push(order);
        }
    }
    orders
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Population;

    fn rule(name: &str) -> BlockRule {
        name.parse().unwrap()
    }

    /// Enough to tell the characters apart, and quick.
    fn glance() -> Effort {
        Effort {
            seeds: 200,
            generations: 1000,
            evaporation: 4000,
        }
    }

    #[test]
    fn known_rules_have_the_character_one_knows_them_by() {
        for (name, character) in [
            ("single-rotation", Character::Spaceships),
            ("espca-01c5ef", Character::Spaceships),
            ("bbm", Character::Gas),
            ("string-thing", Character::Frozen),
            ("espca-0925bf", Character::Explosive),
            ("espca-098aef", Character::Growing),
        ] {
            let report = measure(&rule(name), &glance());
            assert_eq!(report.character(), character, "{name}: {report:?}");
        }
        // Single Rotation is rich: several ships, many periods, and a flip stays a local affair.
        let report = measure(&rule("single-rotation"), &glance());
        assert!(report.spaceships >= 5 && report.periods >= 20, "{report:?}");
        assert!(report.damage < 0.05 && report.remaining > 0.5, "{report:?}");
        // A gas spreads a flip far and loses its blob.
        let gas = measure(&rule("bbm"), &glance());
        assert!(gas.damage > 0.1 && gas.remaining < 0.2, "{gas:?}");
    }

    #[test]
    fn the_blob_finds_ships_the_seeds_do_not() {
        // No seed of six cells is the glider of Critters, but a blob throws them out.
        let seeds_only = Effort { evaporation: 0, ..glance() };
        assert_eq!(measure(&rule("critters"), &seeds_only).spaceships, 0);
        let report = measure(&rule("critters"), &glance());
        assert!(report.spaceships >= 1 && report.caught > 10, "{report:?}");
        assert_eq!(report.character(), Character::Spaceships);
    }

    #[test]
    fn a_report_is_the_same_every_time() {
        let effort = glance();
        for name in ["single-rotation", "critters", "espca-09457f"] {
            assert_eq!(measure(&rule(name), &effort), measure(&rule(name), &effort), "{name}");
        }
    }

    #[test]
    fn the_families_are_as_large_as_they_should_be() {
        let symmetric = rotation_symmetric();
        assert_eq!(symmetric.len(), 1536);
        // Mirror images are one rule, and so is a rule whose vacuum only flickers.
        let distinct_symmetric = distinct(symmetric);
        assert!(distinct_symmetric.len() < 800, "{}", distinct_symmetric.len());
        assert!(distinct_symmetric.iter().all(|rule| rule.representative() == *rule));

        let conserving = conserving();
        assert_eq!(conserving.len(), 829_440);
        let unique: HashSet<&BlockRule> = conserving.iter().collect();
        assert_eq!(unique.len(), conserving.len());
        for rule in conserving.iter().step_by(997) {
            assert_ne!(rule.population(), Population::NotConserved, "{rule}");
        }
        assert_eq!(random(5, 1), random(5, 1));
        assert_eq!(permutations(&[1, 2, 3]).len(), 6);
    }
}
