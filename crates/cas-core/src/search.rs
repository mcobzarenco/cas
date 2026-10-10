//! Looking for rules worth looking at.
//!
//! No one number says that a rule is interesting. What can be measured cheaply is what a rule
//! does not do: it does not freeze, it does not boil, and things in it hold together and move.
//! So a rule is put through a few trials ([`measure`]), the cheap ones first, and its
//! [`Report`] says how each went:
//!
//! * **Seeds.** Small random patterns are left alone on an unbounded plane. Each one comes back
//!   to its shape, in place or elsewhere, or grows without bound, or flies apart.
//! * **Growth.** A seed that grows spreads over the plane like a fire, or it grows along
//!   lines, as a gun does: its cells go with the square of time, or with time.
//! * **Spaceships.** What comes back elsewhere, slower than light, is counted by kind; so is
//!   what a growing seed sends out, and what flies out of a blob through an open border. What
//!   came back, in place or elsewhere, is kept by kind in the report, for whoever keeps it.
//! * **Damage.** One cell of a random soup is flipped. A hundred generations on, how much of
//!   what the flip could have reached is different?
//! * **Blob.** A random blob on a closed grid. Small seeds may all stay small and a blob still
//!   set the grid on fire, so its cells are counted in the end.
//! * **Evaporation.** The same blob with the border open: how much of it is left.
//!
//! Most rules fail the first trial: seeds explode, and such a rule is put through nothing
//! more. Of the others, the ones with several kinds of spaceship, many periods, little damage
//! and a blob that stays a blob are the ones to look at.

use std::collections::{BTreeSet, HashMap};

use rayon::prelude::*;

use crate::{
    census::Census,
    collection::PatternClass,
    pattern::{Analyser, Cell, Fate, Motion},
    rules::{BlockRule, Population},
    universe::{Rng, Universe},
};

/// A seed with more cells than this is growing, and one wider than this has flown apart.
const SEED_CELLS: usize = 120;
const SEED_EXTENT: i32 = 160;
/// So many seeds are looked at first. If a tenth of them grow, no more are followed.
const FIRST_LOOK: usize = 60;
/// How seeds grow is taken from the first few that do, each followed for so many generations
/// on a grid wide enough that nothing gets around it in that time. One that seems to grow
/// along lines is followed again for so many more: a slow starter looks like a gun at first,
/// and spreads over the plane later.
const GROWERS: usize = 6;
const GROWTH_GENERATIONS: i64 = 128;
const LATER_GENERATIONS: i64 = 512;
/// Cells that go with a higher power of time than this are spreading over the plane.
const SPREADING: f32 = 1.5;
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
    /// For how many generations the blob is left alone, on a closed grid and then with the
    /// border open, and a growing seed to send out what it does. With 0 none of it is tried.
    pub blob: i64,
}

impl Default for Effort {
    fn default() -> Self {
        Self { seeds: 400, generations: 3000, blob: 8000 }
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
    /// The power of time that the cells of a growing seed go with: 2 for one that spreads
    /// over the plane, 1 for a gun. Of the first seeds that grow, the slowest: a rule with
    /// guns counts for its guns, whatever else explodes. 0 if no seed grows.
    pub growth: f32,
    /// Kinds of spaceship slower than light: among the seeds, sent out by one that grows, and
    /// from the blob.
    pub spaceships: usize,
    /// How many different periods the oscillating seeds had, and the longest.
    pub periods: usize,
    pub longest_period: u32,
    /// The share that differs of the cells a flipped cell could have reached.
    pub damage: f32,
    /// The cells of a blob on a closed grid in the end, as a multiple of what it began with.
    /// Not tried on a rule whose seeds explode.
    pub blob: Option<f32>,
    /// What is left of the blob with the border open, as a share of its cells. Not tried on a
    /// blob that spreads on the closed grid.
    pub remaining: Option<f32>,
    /// How many spaceships left the blob, and how many small patterns that were none.
    pub caught: u64,
    pub others: u64,
    /// What came back to its shape, by the form its kind is filed under: every kind of
    /// spaceship counted above, and the oscillators and still lifes among the seeds. In the
    /// order of their classes and forms.
    pub found: Vec<Found>,
}

/// A pattern that came back to its shape, as it is kept: its class, the form its kind is filed
/// under, its period, and how far that form moves in a period.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Found {
    pub class: PatternClass,
    pub cells: Vec<Cell>,
    pub period: u32,
    pub moves: (i32, i32),
}

/// What kind of world a rule makes, as far as its trials tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Character {
    /// Nearly every seed grows without bound, spreading over the plane.
    Explosive,
    /// Some seeds do.
    Growing,
    /// Seeds grow without bound, and some only as fast as time goes: guns, puffers, wicks.
    Linear,
    /// Seeds stay small, and yet a blob ends with more cells than fit where it began.
    Igniting,
    /// Something slower than light travels, and nothing gets out of hand.
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
        if self.growing >= 0.1 && self.growth < SPREADING {
            Character::Linear
        } else if self.growing >= 0.9 {
            Character::Explosive
        } else if self.growing >= 0.1 {
            Character::Growing
        } else if self.blob.is_some_and(|cells| cells > 1.0 / BLOB_DENSITY) {
            Character::Igniting
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
    let analyser = || {
        let mut analyser = Analyser::new(rule);
        analyser.max_generations = effort.generations;
        analyser.max_cells = SEED_CELLS;
        analyser.max_extent = SEED_EXTENT;
        analyser
    };
    let all = seeds(effort.seeds);
    let (first, rest) = all.split_at(FIRST_LOOK.min(all.len()));
    let mut seeds = Seeds::new(analyser());
    first.iter().for_each(|seed| seeds.follow(seed));
    // A first look tells whether seeds grow, and more of them would only say so again.
    let grows = seeds.share(seeds.growing) >= 0.1;
    if !grows {
        rest.iter().for_each(|seed| seeds.follow(seed));
    }
    // If all that grow spread over the plane, that is all there is to find out about the rule.
    let slowest = seeds.slowest(rule);
    let explodes = grows && slowest.as_ref().is_some_and(|(_, growth)| *growth >= SPREADING);

    let (mut blob, mut remaining, mut left) = (None, None, (0, 0));
    // A seed that grows along lines may be a gun: its ships are caught at the border.
    if let Some((grower, growth)) = slowest
        && growth < SPREADING
        && effort.blob > 0
    {
        let mut universe = Universe::new(GRID, GRID, rule.clone());
        plant(&mut universe, &grower);
        catch(&mut universe, effort.blob, analyser(), &mut seeds.found);
    }
    if !explodes && effort.blob > 0 {
        let cells = closed(rule, effort.blob);
        blob = Some(cells);
        // A blob that has spread is not there to evaporate.
        if cells <= 1.0 / BLOB_DENSITY {
            let mut universe = blob_of(rule);
            let cells = universe.population();
            left = catch(&mut universe, effort.blob, analyser(), &mut seeds.found);
            remaining = Some(universe.population() as f32 / cells.max(1) as f32);
        }
    }
    let mut found: Vec<Found> = std::mem::take(&mut seeds.found).into_values().collect();
    found.sort();
    Report {
        oscillating: seeds.share(seeds.oscillating),
        travelling: seeds.share(seeds.travelling),
        scattering: seeds.share(seeds.scattering),
        growing: seeds.share(seeds.growing),
        undecided: seeds.share(seeds.undecided),
        growth: seeds.slowest(rule).map_or(0.0, |(_, growth)| growth),
        spaceships: found.iter().filter(|found| found.class == PatternClass::Spaceship).count(),
        periods: seeds.periods.len(),
        longest_period: seeds.periods.last().copied().unwrap_or(0),
        damage: damage(rule),
        blob,
        remaining,
        caught: left.0,
        others: left.1,
        found,
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

/// What became of the seeds followed so far.
struct Seeds {
    analyser: Analyser,
    oscillating: u32,
    travelling: u32,
    scattering: u32,
    growing: u32,
    undecided: u32,
    periods: BTreeSet<u32>,
    /// What came back as one thing, by the form it is filed under.
    found: HashMap<Vec<Cell>, Found>,
    /// The first few that grew and, once it has been looked at, which of them grows most
    /// slowly, with the power of time its cells go with.
    growers: Vec<Vec<Cell>>,
    slowest: Option<(usize, usize, f32)>,
}

impl Seeds {
    fn new(analyser: Analyser) -> Self {
        Self {
            analyser,
            oscillating: 0,
            travelling: 0,
            scattering: 0,
            growing: 0,
            undecided: 0,
            periods: BTreeSet::new(),
            found: HashMap::new(),
            growers: Vec::new(),
            slowest: None,
        }
    }

    fn follow(&mut self, seed: &[Cell]) {
        match self.analyser.fate(seed, 0) {
            Fate::Returns { period, displacement } => {
                if displacement == (0, 0) {
                    self.oscillating += 1;
                    self.periods.insert(period);
                } else {
                    self.travelling += 1;
                }
                // What came back as one thing is a kind of its own: several ships side by
                // side, or oscillators that never meet, are none. A ship as fast as light is
                // no find.
                if let Some(study) = self.analyser.study(seed, 0)
                    && study.parts == 1
                    && let Some(motion) = study.motion
                    && (displacement == (0, 0) || slower_than_light(&motion))
                {
                    let class = PatternClass::of(motion.displacement, study.still);
                    let found =
                        Found { class, period: motion.period, moves: motion.displacement, cells: motion.canonical };
                    self.found.entry(found.cells.clone()).or_insert(found);
                }
            }
            Fate::Scatters => self.scattering += 1,
            Fate::Grows => {
                self.growing += 1;
                if self.growers.len() < GROWERS {
                    self.growers.push(seed.to_vec());
                }
            }
            Fate::Undecided => self.undecided += 1,
        }
    }

    fn share(&self, count: u32) -> f32 {
        let followed = self.oscillating + self.travelling + self.scattering + self.growing + self.undecided;
        count as f32 / followed.max(1) as f32
    }

    /// Of the seeds that grew, the one that does so most slowly, and the power of time its
    /// cells go with. None if no seed grew.
    fn slowest(&mut self, rule: &BlockRule) -> Option<(Vec<Cell>, f32)> {
        // Looked at once, and again only if more seeds have grown since.
        if self.slowest.is_none_or(|(growers, _, _)| growers != self.growers.len()) {
            let growths = self.growers.iter().map(|seed| growth(rule, seed)).enumerate();
            let (seed, growth) = growths.min_by(|a, b| a.1.total_cmp(&b.1))?;
            self.slowest = Some((self.growers.len(), seed, growth));
        }
        self.slowest.map(|(_, seed, growth)| (self.growers[seed].clone(), growth))
    }
}

fn slower_than_light(motion: &Motion) -> bool {
    let (distance, period) = motion.speed();
    distance < period
}

/// Puts a seed in the middle of an empty universe, where it sits on the blocks as before.
fn plant(universe: &mut Universe, seed: &[Cell]) {
    let (x0, y0) = ((universe.width / 2) & !1, (universe.height / 2) & !1);
    for &(x, y) in seed {
        universe.set(x0 + x as usize, y0 + y as usize, true);
    }
}

/// The power of time that the cells of a growing seed go with. That it spreads over the
/// plane shows soon; that it does not is only believed after a longer look.
fn growth(rule: &BlockRule, seed: &[Cell]) -> f32 {
    let soon = growth_over(rule, seed, GROWTH_GENERATIONS);
    if soon >= SPREADING { soon } else { growth_over(rule, seed, LATER_GENERATIONS) }
}

/// The power of time that the cells of a seed go with over so many generations, from their
/// number half way and at the end.
fn growth_over(rule: &BlockRule, seed: &[Cell], generations: i64) -> f32 {
    // Nothing gets further than a cell a generation: no way round a grid twice as wide.
    let side = 2 * generations as usize + 32;
    let mut universe = Universe::new(side, side, rule.clone());
    plant(&mut universe, seed);
    universe.step_by(generations / 2);
    let half_way = universe.population().max(1) as f32;
    universe.step_by(generations / 2);
    (universe.population().max(1) as f32 / half_way).log2()
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

/// The blob every rule is tried on.
fn blob_of(rule: &BlockRule) -> Universe {
    let mut universe = Universe::new(GRID, GRID, rule.clone());
    universe.randomize_blob(BLOB_DENSITY, &mut Rng::new(42));
    universe
}

/// Leaves the blob alone on a closed grid: its cells in the end, as a multiple of what it
/// began with.
fn closed(rule: &BlockRule, generations: i64) -> f32 {
    // Under a rule that keeps the number of cells there is nothing to find out.
    if matches!(rule.population(), Population::Conserved | Population::ConservedRelativeToVacuum) {
        return 1.0;
    }
    let mut universe = blob_of(rule);
    let cells = universe.population();
    universe.step_by(generations);
    universe.population() as f32 / cells.max(1) as f32
}

/// Leaves a universe alone with its border open, and identifies the small patterns that
/// leave. The kinds of the slow spaceships among them join what was `found`; returns how many
/// spaceships left, and how many small patterns that were none.
fn catch(
    universe: &mut Universe,
    generations: i64,
    analyser: Analyser,
    found: &mut HashMap<Vec<Cell>, Found>,
) -> (u64, u64) {
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
    for kind in census.kinds().iter().filter(|kind| slower_than_light(&kind.motion)) {
        let motion = &kind.motion;
        let ship = Found {
            class: PatternClass::Spaceship,
            cells: motion.canonical.clone(),
            period: motion.period,
            moves: motion.displacement,
        };
        found.entry(ship.cells.clone()).or_insert(ship);
    }
    (census.ships(), census.others())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::Heading;

    fn rule(name: &str) -> BlockRule {
        name.parse().unwrap()
    }

    /// Enough to tell the characters apart, and quick.
    fn glance() -> Effort {
        Effort { seeds: 200, generations: 1000, blob: 4000 }
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
        assert!(report.damage < 0.05 && report.remaining.is_some_and(|left| left > 0.5), "{report:?}");
        assert_eq!((report.blob, report.growth), (Some(1.0), 0.0));
        // A gas spreads a flip far and loses its blob.
        let gas = measure(&rule("bbm"), &glance());
        assert!(gas.damage > 0.1 && gas.remaining.is_some_and(|left| left < 0.2), "{gas:?}");
    }

    #[test]
    fn a_rule_that_explodes_is_put_through_nothing_more() {
        // The disk of ESPCA-0925bf: every seed spreads over the plane.
        let report = measure(&rule("espca-0925bf"), &glance());
        assert!(report.growing >= 0.9 && report.growth > 1.8, "{report:?}");
        assert_eq!((report.blob, report.remaining), (None, None));
        // What is known of it comes from the seeds of the first look.
        let fewer = Effort { seeds: FIRST_LOOK, ..glance() };
        assert_eq!(report, measure(&rule("espca-0925bf"), &fewer));
    }

    #[test]
    fn a_rule_with_guns_counts_for_its_guns() {
        // In ESPCA-09457f a lone cell is a gun, whatever larger seeds do; and the same holds
        // for the rule seen in a mirror, where other seeds come first.
        let gun = rule("espca-09457f");
        for rule in [gun.canonical(), gun] {
            let report = measure(&rule, &glance());
            assert_eq!(report.character(), Character::Linear, "{rule}: {report:?}");
            assert!(report.growing >= 0.5 && (0.8..1.2).contains(&report.growth), "{rule}: {report:?}");
            assert!(report.spaceships >= 1 && report.blob.is_some(), "{rule}: {report:?}");
        }
    }

    #[test]
    fn a_slow_starter_is_no_gun() {
        // The seeds of ESPCA-0c8adf grow slowly at first: one of them by the power 0.7 of time
        // between generations 64 and 128, as a gun would. By generation 600 every one of
        // them goes with the square of time, and has for a while.
        let slow = rule("0,3,10,1,5,4,6,7,12,9,2,11,8,13,14,15");
        assert_eq!(slow.espca().as_deref(), Some("0c8adf"));
        let report = measure(&slow, &glance());
        assert!(report.growing >= 0.5 && report.growth >= SPREADING, "{report:?}");
        assert_eq!(report.character(), Character::Growing, "{report:?}");
    }

    #[test]
    fn seeds_alone_do_not_tell_whether_a_blob_keeps_still() {
        // Single Rotation, but a block of three cells fills up and a full one loses a cell.
        // Small seeds hardly ever get that dense: they oscillate and travel as they did.
        let igniting = rule("0,2,8,3,1,5,6,7,4,9,10,11,12,13,15,14");
        let report = measure(&igniting, &glance());
        assert!(report.growing < 0.1 && report.spaceships > 0 && report.periods >= 10, "{report:?}");
        assert!(report.blob.is_some_and(|cells| cells > 20.0), "{report:?}");
        assert_eq!((report.character(), report.remaining), (Character::Igniting, None));

        // A rule that makes and unmakes cells, and whose blob stays a blob all the same.
        let tame = rule("0,1,11,5,13,12,15,14,8,9,3,2,10,4,7,6");
        assert_eq!(tame.population(), Population::NotConserved);
        let report = measure(&tame, &glance());
        assert_eq!(report.character(), Character::Spaceships, "{report:?}");
        assert!(report.blob.is_some_and(|cells| (1.2..2.5).contains(&cells)), "{report:?}");
        assert!(report.remaining.is_some_and(|left| left > 1.0), "{report:?}");
    }

    #[test]
    fn the_blob_finds_ships_the_seeds_do_not() {
        // No seed of six cells is the glider of Critters, but a blob throws them out.
        let seeds_only = Effort { blob: 0, ..glance() };
        assert_eq!(measure(&rule("critters"), &seeds_only).spaceships, 0);
        let report = measure(&rule("critters"), &glance());
        assert!(report.spaceships >= 1 && report.caught > 10, "{report:?}");
        assert_eq!(report.character(), Character::Spaceships);
    }

    /// The slow spaceships that a blob sends out through an open border in so many generations.
    fn fleet(rule: &BlockRule, generations: i64) -> Vec<Motion> {
        let mut universe = blob_of(rule);
        universe.open_border = true;
        universe.catching = true;
        let mut census = Census::new(rule);
        for _ in 0..generations / 64 {
            universe.step_by(64);
            universe.take_departures().into_iter().for_each(|departure| census.record(departure));
        }
        census.kinds().iter().map(|kind| kind.motion.clone()).filter(slower_than_light).collect()
    }

    #[test]
    fn the_rules_the_search_found_do_what_their_blurbs_say() {
        let found = |id: &str| measure(&rule(id), &Effort::default());
        // Cells are made and unmade, and a blob stays a blob: at one and a half times its
        // cells, or creeping on to twice as many.
        let (steady, creeping) = (found("steady-blob"), found("creeping-blob"));
        for report in [&steady, &creeping] {
            assert_eq!(report.character(), Character::Spaceships, "{report:?}");
            assert!(report.oscillating > 0.95 && report.periods >= 20, "{report:?}");
        }
        assert!(steady.blob.is_some_and(|cells| (1.3..1.6).contains(&cells)), "{steady:?}");
        assert!(creeping.blob > steady.blob && creeping.spaceships > steady.spaceships, "{creeping:?}");
        let for_long = Effort { seeds: FIRST_LOOK, blob: 30_000, ..Effort::default() };
        let (steady, creeping) = (measure(&rule("steady-blob"), &for_long), measure(&rule("creeping-blob"), &for_long));
        assert!(steady.blob.is_some_and(|cells| (1.3..1.6).contains(&cells)), "{steady:?}");
        assert!(creeping.blob.is_some_and(|cells| (1.8..2.2).contains(&cells)), "{creeping:?}");

        let factory = found("ship-factory");
        assert_eq!(factory.character(), Character::Spaceships, "{factory:?}");
        assert!(factory.caught >= 100 && factory.remaining.is_some_and(|left| left > 1.0), "{factory:?}");
        let small = |ship: &Motion| ship.canonical.len() <= 8 && [(1, 3), (1, 7)].contains(&ship.speed());
        assert!(fleet(&rule("ship-factory"), 8000).iter().all(small));

        // Every one of them flies the same way, at a sixth of the speed of light.
        let plus = fleet(&rule("plus-ships"), 8000);
        assert!(
            !plus.is_empty() && plus.iter().all(|ship| ship.speed() == (1, 6) && ship.heading() == Heading::Diagonal)
        );
        assert_eq!(found("plus-ships").character(), Character::Spaceships);

        let gun = measure(&rule("four-way-gun"), &glance());
        assert_eq!(gun.character(), Character::Linear, "{gun:?}");
        assert!(gun.spaceships >= 1 && gun.blob.is_some_and(|cells| cells > 20.0), "{gun:?}");
        // A lone cell, a while on: cells along all four diagonals, and nowhere else.
        let mut universe = Universe::new(GRID, GRID, rule("four-way-gun"));
        plant(&mut universe, &[(0, 0)]);
        universe.step_by(200);
        let mut arms = BTreeSet::new();
        for (index, _) in universe.cells().iter().enumerate().filter(|(_, cell)| **cell != 0) {
            let (dx, dy) = ((index % GRID) as i32 - (GRID / 2) as i32, (index / GRID) as i32 - (GRID / 2) as i32);
            assert!((dx.abs() - dy.abs()).abs() <= 8, "a cell off the diagonals: {dx}, {dy}");
            if dx.abs() > 8 {
                arms.insert((dx.signum(), dy.signum()));
            }
        }
        assert_eq!(arms.len(), 4);

        // The directions the ships of a blob fly in.
        let ways = |id: &str| -> BTreeSet<(i32, i32)> {
            let towards = |ship: &Motion| (ship.displacement.0.signum(), ship.displacement.1.signum());
            fleet(&rule(id), 8000).iter().map(towards).collect()
        };
        assert_eq!(ways("crossing-fleets"), BTreeSet::from([(-1, -1), (-1, 1), (1, -1), (1, 1)]));
        assert_eq!(ways("diagonal-traffic"), BTreeSet::from([(-1, -1), (1, 1)]));
        assert!(fleet(&rule("diagonal-traffic"), 24_000).len() >= 12);
        assert!(found("crossing-fleets").periods >= 40);
    }

    #[test]
    fn a_report_is_the_same_every_time() {
        let effort = glance();
        for name in ["single-rotation", "critters", "espca-09457f"] {
            assert_eq!(measure(&rule(name), &effort), measure(&rule(name), &effort), "{name}");
        }
    }
}
