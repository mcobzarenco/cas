//! Families of rules: the places a search can look.
//!
//! There are 16! reversible rules, far too many to go through, and nearly all of them turn
//! everything into noise. A family is the set of rules that have some properties in common
//! ([`Constraint`]s), chosen to hold the noise off: a symmetry, something the rule conserves,
//! a form its table has. Any properties may be asked for together, and the family is then the
//! rules that have them all; asked for none, it is every rule there is.
//!
//! A family is enumerated by filling in the table a block at a time and striking out what its
//! properties forbid as soon as that can be seen ([`Family::rules`]). One too big for that is
//! sampled ([`Family::sample`]).

use std::{collections::HashSet, fmt, str::FromStr};

use crate::{
    rules::{
        BlockRule, Population, TURNS_AND_MIRRORS, anti_transpose, complement, flip, keeps_weight, mirror, popcount,
        rotate_180, rotate_cw, transpose, weigh, weightings,
    },
    universe::Rng,
};

/// A way to turn or mirror the plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Turn {
    Quarter,
    Half,
    /// Left to right.
    Mirror,
    /// Top to bottom.
    Flip,
    /// Across the diagonal from top-left to bottom-right.
    Diagonal,
    AntiDiagonal,
}

impl Turn {
    /// What the turn does to a block.
    pub fn transform(self) -> fn(u8) -> u8 {
        match self {
            Turn::Quarter => rotate_cw,
            Turn::Half => rotate_180,
            Turn::Mirror => mirror,
            Turn::Flip => flip,
            Turn::Diagonal => transpose,
            Turn::AntiDiagonal => anti_transpose,
        }
    }
}

/// A property that the rules of a family have in common. Whatever is about the cells of a
/// pattern is judged relative to the vacuum, as [`BlockRule::population`] judges it: Critters
/// conserves cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Constraint {
    /// The rule looks the same after the plane is turned or mirrored so.
    Symmetric(Turn),
    /// Patterns keep their number of cells.
    Conserving,
    /// Patterns keep a weighted number of cells and not their number, a cell weighing by its
    /// corner of the block about to be rewritten (top-left, top-right, bottom-left,
    /// bottom-right). Given no weights, any weighting will do.
    Weighted(Option<[u8; 4]>),
    /// Patterns keep the parity of their number of cells.
    Parity,
    /// Patterns keep their momentum. A cell's corner says which way it is going, as in
    /// Morita's ESPCAs (top-left east, top-right south, bottom-left north, bottom-right west),
    /// and the cells going east less those going west, and north less south, never change in
    /// number.
    Momentum,
    /// Exchanging dead and alive turns every run into another run.
    Complement,
    /// The rule is its own inverse: run backwards, it is the same rule.
    Involution,
    /// The empty world stays empty.
    StableVacuum,
    /// Every block becomes a turn or a mirror of itself.
    Turning,
    /// The rule changes at most so many of the sixteen blocks.
    Sparse(u8),
    /// The rule is linear, that is affine over the field of two elements: patterns superpose.
    Linear,
}

/// The names the constraints go by, and what each says of its rules. [`Constraint::Weighted`]
/// with weights and [`Constraint::Sparse`] take a value after `=`.
const NAMES: [(&str, Constraint, &str); 16] = [
    (
        "quarter-turn",
        Constraint::Symmetric(Turn::Quarter),
        "The 1536 rules that look the same after a quarter turn: Morita's ESPCAs",
    ),
    ("half-turn", Constraint::Symmetric(Turn::Half), "The 1 105 920 rules that look the same after a half turn"),
    (
        "mirror",
        Constraint::Symmetric(Turn::Mirror),
        "The 1 105 920 rules that look the same in a mirror, left to right",
    ),
    (
        "flip",
        Constraint::Symmetric(Turn::Flip),
        "The rules that look the same in a mirror, top to bottom: the worlds of `mirror`, turned",
    ),
    (
        "diagonal",
        Constraint::Symmetric(Turn::Diagonal),
        "The 15 482 880 rules that look the same in a mirror across the diagonal from the top-left",
    ),
    (
        "anti-diagonal",
        Constraint::Symmetric(Turn::AntiDiagonal),
        "The rules that look the same in a mirror across the other diagonal: the worlds of `diagonal`, turned",
    ),
    (
        "conserving",
        Constraint::Conserving,
        "The 845 040 rules under which patterns keep their number of cells, as seen against the vacuum: Critters is one",
    ),
    (
        "weighted",
        Constraint::Weighted(None),
        "The 216 480 rules under which patterns keep a weighted number of cells and not their number: cells are made \
         and unmade, yet nothing can explode",
    ),
    ("parity", Constraint::Parity, "The rules under which patterns keep the parity of their number of cells"),
    (
        "momentum",
        Constraint::Momentum,
        "The 228 rules under which patterns keep their momentum, a cell's corner being the way it is going",
    ),
    ("complement", Constraint::Complement, "The rules for which dead and alive are interchangeable"),
    ("involution", Constraint::Involution, "The rules that are their own inverse"),
    ("stable-vacuum", Constraint::StableVacuum, "The rules that leave the empty world empty"),
    ("turning", Constraint::Turning, "The 27 648 rules that make every block a turn or a mirror of itself"),
    ("sparse", Constraint::Sparse(4), "The rules that change at most N of the 16 blocks; 4 unless said"),
    ("linear", Constraint::Linear, "The 322 560 rules under which patterns superpose"),
];

/// What may be written for a family, as the help of a program lists it: every constraint as
/// it is typed, with what it says of its rules.
pub fn catalogue() -> Vec<(&'static str, &'static str)> {
    let mut catalogue = Vec::new();
    for (name, constraint, about) in NAMES {
        match constraint {
            Constraint::Sparse(_) => catalogue.push(("sparse=N", about)),
            _ => catalogue.push((name, about)),
        }
        if constraint == Constraint::Weighted(None) {
            catalogue.push((
                "weights=A,B,C,D",
                "The rules under which patterns keep their cells weighted so: a cell counts A in the top-left corner \
                 of its block, B top-right, C bottom-left, D bottom-right",
            ));
        }
    }
    catalogue.push(("random", "Every rule there is: nothing is required"));
    catalogue
}

impl Constraint {
    /// Does the rule have the property? The truth, whatever the search guessed on the way.
    pub fn holds(&self, rule: &BlockRule) -> bool {
        let relative = || rule.relative_to_vacuum();
        let table = rule.table();
        match *self {
            Constraint::Symmetric(turn) => rule.commutes_with(turn.transform()),
            Constraint::Conserving => {
                matches!(rule.population(), Population::Conserved | Population::ConservedRelativeToVacuum)
            }
            Constraint::Weighted(Some(weights)) => relative().iter().all(|rule| keeps_weight(rule.table(), &weights)),
            Constraint::Weighted(None) => matches!(rule.population(), Population::Weighted(_)),
            Constraint::Parity => relative()
                .iter()
                .all(|rule| (0..16u8).all(|block| popcount(rule.table()[block as usize]) % 2 == popcount(block) % 2)),
            Constraint::Momentum => relative().iter().all(|rule| {
                (0..16u8).all(|block| momentum(rotate_180(rule.table()[block as usize])) == momentum(block))
            }),
            Constraint::Complement => rule.is_complement_symmetric(),
            Constraint::Involution => (0..16).all(|block| table[table[block] as usize] == block as u8),
            Constraint::StableVacuum => table[0] == 0,
            Constraint::Turning => (0..16).all(|block| orbit(block as u8) >> table[block] & 1 == 1),
            Constraint::Sparse(most) => (0..16).filter(|&block| table[block] != block as u8).count() <= most as usize,
            Constraint::Linear => (0..16).all(|a| (0..16).all(|b| table[a ^ b] == table[a] ^ table[b] ^ table[0])),
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Constraint::Weighted(Some(w)) => write!(f, "weights={},{},{},{}", w[0], w[1], w[2], w[3]),
            Constraint::Sparse(most) => write!(f, "sparse={most}"),
            other => {
                let (name, ..) =
                    NAMES.iter().find(|(_, known, _)| *known == other).expect("every constraint has a name");
                f.write_str(name)
            }
        }
    }
}

impl FromStr for Constraint {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        if let Some((name, value)) = text.split_once('=') {
            return match name.trim() {
                "sparse" => value
                    .trim()
                    .parse::<u8>()
                    .ok()
                    .filter(|most| *most <= 16)
                    .map(Constraint::Sparse)
                    .ok_or_else(|| format!("sparse={value:?}: how many blocks may change, 0 to 16")),
                "weights" => {
                    let weights: Vec<u8> = value.split(',').filter_map(|w| w.trim().parse().ok()).collect();
                    match weights[..] {
                        [a, b, c, d] if weights.iter().all(|w| (1..=9).contains(w)) => {
                            Ok(Constraint::Weighted(Some([a, b, c, d])))
                        }
                        _ => Err(format!(
                            "weights={value:?}: four weights from 1 to 9, top-left, top-right, bottom-left, bottom-right"
                        )),
                    }
                }
                other => Err(format!("{other:?} takes no value")),
            };
        }
        NAMES.iter().find(|(name, ..)| *name == text).map(|(_, constraint, _)| *constraint).ok_or_else(|| {
            let names: Vec<&str> = catalogue().into_iter().map(|(name, _)| name).collect();
            format!("no property of rules is called {text:?}; there are {}", names.join(", "))
        })
    }
}

/// The rules that have some properties in common.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Family {
    constraints: Vec<Constraint>,
}

/// So many rules a family may have and still be gone through one by one.
pub const ENUMERABLE: usize = 8_000_000;
/// After so many draws in a row that found no rule, a sample is given up on.
const GIVEN_UP: usize = 1000;

impl Family {
    pub fn new(constraints: impl IntoIterator<Item = Constraint>) -> Self {
        let mut family = Self::default();
        for constraint in constraints {
            if !family.constraints.contains(&constraint) {
                family.constraints.push(constraint);
            }
        }
        family
    }

    /// Reads a family from names joined by `+`, such as `mirror+conserving`; `random` is the
    /// family with no properties required, every rule, and may be joined with anything.
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts = text.split('+').map(str::trim).filter(|part| *part != "random" && !part.is_empty());
        Ok(Self::new(parts.map(str::parse).collect::<Result<Vec<_>, _>>()?))
    }

    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// Is the rule one of the family?
    pub fn holds(&self, rule: &BlockRule) -> bool {
        self.constraints.iter().all(|constraint| constraint.holds(rule))
    }

    /// How many rules the family has, counted up to `cap`: `None` when there are more than
    /// that, as there always are when nothing is required.
    pub fn count(&self, cap: usize) -> Option<usize> {
        if self.constraints.is_empty() {
            return None;
        }
        if self.constraints.contains(&Constraint::Weighted(None)) {
            return Some(self.rules().len()).filter(|count| *count <= cap);
        }
        let mut count = 0;
        Filler::new(&self.constraints, None).fill(&mut |_| {
            count += 1;
            count < cap
        });
        Some(count).filter(|count| *count < cap)
    }

    /// Every rule of the family, for a family whose [`Family::count`] fits.
    pub fn rules(&self) -> Vec<BlockRule> {
        if self.constraints.is_empty() {
            return Vec::new();
        }
        // Any weighting will do: the families of every weighting but the plain one, together.
        if let Some(place) = self.constraints.iter().position(|c| *c == Constraint::Weighted(None)) {
            let mut seen = HashSet::new();
            let mut rules = Vec::new();
            for weights in weightings().into_iter().filter(|weights| *weights != [1; 4]) {
                let mut constraints = self.constraints.clone();
                constraints[place] = Constraint::Weighted(Some(weights));
                for rule in Family::new(constraints).rules() {
                    if Constraint::Weighted(None).holds(&rule) && seen.insert(rule.clone()) {
                        rules.push(rule);
                    }
                }
            }
            return rules;
        }
        let mut rules = Vec::new();
        Filler::new(&self.constraints, None).fill(&mut |rule| {
            rules.push(rule);
            true
        });
        rules
    }

    /// So many rules of the family drawn at random, for a family too big to go through.
    pub fn sample(&self, count: usize, seed: u64) -> Vec<BlockRule> {
        let mut rng = Rng::new(seed);
        let mut rules = Vec::with_capacity(count);
        // A draw fails when the weighting drawn has no rule of its own in the family; a
        // family with no rule at all fails every time, and is given up on.
        let mut failures = 0;
        while rules.len() < count && failures < GIVEN_UP {
            match self.draw(&mut rng) {
                Some(rule) => {
                    rules.push(rule);
                    failures = 0;
                }
                None => failures += 1,
            }
        }
        rules
    }

    /// One rule of the family drawn at random, or none if the draw found none: a family can
    /// be empty, and where any weighting will do, one is drawn first that may have no rule
    /// of its own.
    ///
    /// The table is filled in block by block with the outcomes tried in a random order, so
    /// every outcome a block can have is as likely as any other, however many rules lie
    /// behind it: the draw is not even. Of the rules that are their own inverse, more than a
    /// fifth leave the empty block empty, and one draw in sixteen does.
    pub fn draw(&self, rng: &mut Rng) -> Option<BlockRule> {
        if self.constraints.is_empty() {
            return Some(BlockRule::random(|| rng.next_u64()));
        }
        let mut constraints = self.constraints.clone();
        let weighted = constraints.iter().position(|c| *c == Constraint::Weighted(None));
        if let Some(place) = weighted {
            let weightings: Vec<[u8; 4]> = weightings().into_iter().filter(|weights| *weights != [1; 4]).collect();
            constraints[place] =
                Constraint::Weighted(Some(weightings[(rng.next_u64() % weightings.len() as u64) as usize]));
        }
        let mut found = None;
        Filler::new(&constraints, Some(rng)).fill(&mut |rule| {
            found = Some(rule);
            false
        });
        found.filter(|rule| weighted.is_none() || Constraint::Weighted(None).holds(rule))
    }
}

/// One rule for each set of rules that differ only in how one looks at them
/// ([`BlockRule::canonical`]), in the order they first come up.
pub fn distinct(rules: impl IntoIterator<Item = BlockRule>) -> Vec<BlockRule> {
    let mut seen = HashSet::new();
    let canonical = rules.into_iter().map(|rule| rule.canonical());
    canonical.filter(|rule| seen.insert(rule.clone())).collect()
}

/// A cell's momentum by its corner of the block about to be rewritten, as Morita reads it: the
/// cells going east less those going west, and north less south.
fn momentum(block: u8) -> (i8, i8) {
    let (tl, tr, bl, br) = ((block & 1) as i8, (block >> 1 & 1) as i8, (block >> 2 & 1) as i8, (block >> 3 & 1) as i8);
    (tl - br, bl - tr)
}

/// The blocks that are turns or mirrors of a block, itself included, as a set of bits.
fn orbit(block: u8) -> u16 {
    TURNS_AND_MIRRORS.iter().fold(1 << block, |orbit, turn| orbit | 1 << turn(block))
}

/// No outcome yet.
const NONE: u8 = 16;

/// What was done to the table, to be undone.
enum Step {
    Assigned(u8),
    /// The vacuum's cycle had so many states, and was or was not closed.
    Vacuum(usize, bool),
}

/// Fills in a table under constraints, block by block, striking out what they forbid as soon
/// as it can be seen; what is left at the end is checked against the constraints in full.
struct Filler<'a> {
    constraints: &'a [Constraint],
    table: [u8; 16],
    /// Which outcomes are taken.
    used: u16,
    /// How many blocks the table changes so far.
    moved: u8,
    /// The states of the vacuum's cycle, from the empty block on, as far as they are known,
    /// and whether the cycle has come back to the empty block. The cycle is filled in first,
    /// so that what is judged relative to the vacuum can be judged as the table grows.
    vacuum: Vec<u8>,
    closed: bool,
    trail: Vec<Step>,
    /// Shuffles the outcomes tried, for a sample.
    rng: Option<&'a mut Rng>,
}

impl<'a> Filler<'a> {
    fn new(constraints: &'a [Constraint], rng: Option<&'a mut Rng>) -> Self {
        Self {
            constraints,
            table: [NONE; 16],
            used: 0,
            moved: 0,
            vacuum: vec![0],
            closed: false,
            trail: Vec::new(),
            rng,
        }
    }

    /// Goes through every way to finish the table, handing each rule to `found` until it
    /// says to stop. Returns whether it got to the end.
    fn fill(&mut self, found: &mut dyn FnMut(BlockRule) -> bool) -> bool {
        let Some(block) = self.next() else {
            let rule = BlockRule::new(self.table).expect("every outcome is taken once");
            let holds = self.constraints.iter().all(|constraint| constraint.holds(&rule));
            return !holds || found(rule);
        };
        let mut outcomes: Vec<u8> = (0..16).filter(|outcome| self.used >> outcome & 1 == 0).collect();
        if let Some(rng) = self.rng.as_deref_mut() {
            for i in (1..outcomes.len()).rev() {
                outcomes.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
            }
        }
        for outcome in outcomes {
            let mark = self.trail.len();
            let go_on = !self.assign(block, outcome) || self.fill(found);
            self.undo(mark);
            if !go_on {
                return false;
            }
        }
        true
    }

    /// The block to fill in next: the vacuum's cycle first, then the rest in order.
    fn next(&self) -> Option<u8> {
        if !self.closed {
            let last = *self.vacuum.last().expect("the cycle begins with the empty block");
            if self.table[last as usize] == NONE {
                return Some(last);
            }
        }
        (0..16u8).find(|&block| self.table[block as usize] == NONE)
    }

    /// Gives a block its outcome, with everything that follows from it: the outcomes of the
    /// blocks tied to it by a symmetry, its own under an involution, its complement's, the
    /// sums of a linear rule. Returns false if the constraints forbid it.
    fn assign(&mut self, block: u8, outcome: u8) -> bool {
        let mut pending = vec![(block, outcome)];
        while let Some((block, outcome)) = pending.pop() {
            let known = self.table[block as usize];
            if known != NONE {
                if known != outcome {
                    return false;
                }
                continue;
            }
            if self.used >> outcome & 1 == 1 || !self.allows(block, outcome) {
                return false;
            }
            self.table[block as usize] = outcome;
            self.used |= 1 << outcome;
            self.moved += (block != outcome) as u8;
            self.trail.push(Step::Assigned(block));
            for constraint in self.constraints {
                match *constraint {
                    Constraint::Symmetric(turn) => {
                        let transform = turn.transform();
                        pending.push((transform(block), transform(outcome)));
                    }
                    Constraint::Complement => pending.push((complement(block), complement(outcome))),
                    Constraint::Involution => pending.push((outcome, block)),
                    Constraint::Linear if self.table[0] != NONE => {
                        let origin = self.table[0];
                        for other in 0..16u8 {
                            if self.table[other as usize] != NONE {
                                pending.push((other ^ block, self.table[other as usize] ^ outcome ^ origin));
                            }
                        }
                    }
                    _ => {}
                }
            }
            if !self.closed && block == *self.vacuum.last().expect("the cycle begins with the empty block") {
                self.trail.push(Step::Vacuum(self.vacuum.len(), self.closed));
                self.extend_vacuum();
            }
        }
        true
    }

    /// Follows the vacuum's cycle as far as the table is filled in: the empty world's next
    /// state is the empty block's outcome, seen from the blocks of the next step.
    fn extend_vacuum(&mut self) {
        loop {
            let last = *self.vacuum.last().expect("the cycle begins with the empty block");
            let outcome = self.table[last as usize];
            if outcome == NONE {
                return;
            }
            let next = rotate_180(outcome);
            if next == 0 {
                self.closed = true;
                return;
            }
            self.vacuum.push(next);
        }
    }

    /// Would the constraints allow the block that outcome, as far as can be told now? What
    /// is judged relative to the vacuum is judged against the states of its cycle filled in
    /// so far; the rest is for the check of the whole table.
    fn allows(&self, block: u8, outcome: u8) -> bool {
        let relative = |same: &dyn Fn(u8, u8) -> bool| {
            self.vacuum.iter().all(|&vacuum| {
                let after = self.table[vacuum as usize];
                after == NONE || same(outcome ^ after, block ^ vacuum)
            })
        };
        self.constraints.iter().all(|constraint| match *constraint {
            Constraint::Conserving => relative(&|out, before| popcount(out) == popcount(before)),
            Constraint::Weighted(Some(weights)) => {
                relative(&|out, before| weigh(rotate_180(out), &weights) == weigh(before, &weights))
            }
            Constraint::Parity => relative(&|out, before| popcount(out) % 2 == popcount(before) % 2),
            Constraint::Momentum => relative(&|out, before| momentum(rotate_180(out)) == momentum(before)),
            Constraint::StableVacuum => block != 0 || outcome == 0,
            Constraint::Turning => orbit(block) >> outcome & 1 == 1,
            Constraint::Sparse(most) => {
                // A block whose own place is taken will have to change as well.
                let displaced = (0..16u8).filter(|&other| {
                    let taken = self.used >> other & 1 == 1 || other == outcome;
                    other != block && self.table[other as usize] == NONE && taken
                });
                self.moved + (block != outcome) as u8 + displaced.count() as u8 <= most
            }
            _ => true,
        })
    }

    fn undo(&mut self, mark: usize) {
        while self.trail.len() > mark {
            match self.trail.pop().expect("the trail is longer than the mark") {
                Step::Assigned(block) => {
                    let outcome = self.table[block as usize];
                    self.table[block as usize] = NONE;
                    self.used &= !(1 << outcome);
                    self.moved -= (block != outcome) as u8;
                }
                Step::Vacuum(states, closed) => {
                    self.vacuum.truncate(states);
                    self.closed = closed;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{PRESETS, Population, Preset};

    fn family(text: &str) -> Family {
        Family::parse(text).unwrap()
    }

    fn preset(id: &str) -> BlockRule {
        PRESETS.iter().find(|preset| preset.id == id).map(Preset::rule).unwrap()
    }

    #[test]
    fn the_symmetric_families_are_the_rules_with_that_symmetry() {
        let turning = family("quarter-turn").rules();
        assert_eq!(turning.len(), 1536);
        assert!(turning.iter().all(|rule| rule.espca().is_some()));
        let unique: HashSet<&BlockRule> = turning.iter().collect();
        assert_eq!(unique.len(), turning.len());
        // Mirror images are one rule, and so are a rule whose vacuum only flickers and a
        // rule begun a generation later.
        let distinct_turning = distinct(turning);
        assert_eq!(distinct_turning.len(), 584);
        assert!(distinct_turning.iter().all(|rule| rule.canonical() == *rule));

        for (name, transform) in [("half-turn", rotate_180 as fn(u8) -> u8), ("mirror", mirror), ("flip", flip)] {
            let family = family(name);
            assert_eq!(family.count(ENUMERABLE), Some(1_105_920), "{name}");
            let rules = family.rules();
            assert_eq!(rules.len(), 1_105_920, "{name}");
            assert!(rules.iter().step_by(997).all(|rule| rule.commutes_with(transform)));
            let unique: HashSet<&BlockRule> = rules.iter().collect();
            assert_eq!(unique.len(), rules.len());
        }
        // A diagonal mirror fixes two corners of a block, so more rules have it: too many to
        // go through, and a different family from the mirror's, not its image.
        assert_eq!(family("diagonal").count(ENUMERABLE), None);
        assert_eq!(family("diagonal").count(20_000_000), Some(15_482_880));
        // Together, symmetries make the group they generate.
        assert_eq!(family("mirror+flip").rules().len(), 512);
        assert_eq!(family("quarter-turn+mirror").rules().len(), 64);
        assert!(
            family("quarter-turn+mirror").rules().iter().all(|rule| rule.symmetry() == crate::rules::Symmetry::Full)
        );
    }

    #[test]
    fn the_conserving_family_keeps_the_cells_of_a_pattern() {
        let conserving = family("conserving");
        let rules = conserving.rules();
        assert_eq!(conserving.count(ENUMERABLE), Some(rules.len()));
        // The rules that keep the number of cells of every block, and the ones that trade
        // it for the number of dead cells over a vacuum that flips, as Critters does; and a
        // few more over vacuums that go through other cycles.
        assert_eq!(rules.len(), 845_040);
        let unique: HashSet<&BlockRule> = rules.iter().collect();
        assert_eq!(unique.len(), rules.len());
        for rule in rules.iter().step_by(997) {
            assert!(
                matches!(rule.population(), Population::Conserved | Population::ConservedRelativeToVacuum),
                "{rule}"
            );
            assert_eq!(rule.conserved_weights(), Some([1; 4]), "{rule}");
        }
        assert!(rules.contains(&preset("critters")) && rules.contains(&preset("single-rotation")));
        let strict = rules.iter().filter(|rule| rule.population() == Population::Conserved).count();
        assert_eq!(strict, 414_720);
    }

    #[test]
    fn the_weighted_family_keeps_a_weight_and_not_the_cells() {
        let weighted = family("weighted").rules();
        let unique: HashSet<&BlockRule> = weighted.iter().collect();
        assert_eq!(unique.len(), weighted.len());
        // Half of them leave the empty world empty; the others keep their weight relative to
        // a vacuum that goes through a cycle.
        assert_eq!(weighted.len(), 216_480);
        assert_eq!(weighted.iter().filter(|rule| rule.table()[0] == 0).count(), 107_664);
        assert_eq!(distinct(weighted.clone()).len(), 20_729);
        for rule in weighted.iter().step_by(97) {
            let Population::Weighted(weights) = rule.population() else {
                panic!("{rule} keeps no weight");
            };
            assert!(weights.contains(&1) && weights != [1; 4], "{rule}: {weights:?}");
        }
        // One of them: the bottom-right cell of a block counts double. Alone it comes apart
        // into the two cells of the other diagonal, and the two cells of the top row become
        // one, top-left: that is bottom-right in the block the next step rewrites.
        let rule: BlockRule = "0,8,2,1,4,10,12,14,6,3,5,7,9,11,13,15".parse().unwrap();
        assert_eq!((rule.table()[8], rule.table()[3]), (6, 1));
        assert_eq!(rule.conserved_weights(), Some([1, 1, 1, 2]));
        assert!(weighted.contains(&rule));
        assert!(family("weights=1,1,1,2").rules().contains(&rule));
        // Most rules keep no weight at all.
        assert_eq!("espca-0925bf".parse::<BlockRule>().unwrap().conserved_weights(), None);
    }

    #[test]
    fn the_other_properties_single_out_the_rules_known_for_them() {
        // Momentum: the HPP gas has it, and so does swapping the cells of a diagonal, which is
        // linear besides.
        let momentum = family("momentum").rules();
        assert_eq!(momentum.len(), 228);
        assert_eq!(distinct(momentum.clone()).len(), 41);
        assert!(momentum.contains(&preset("hpp-gas")) && momentum.contains(&preset("swap-on-diagonal")));
        assert!(family("linear").holds(&preset("swap-on-diagonal")));
        assert_eq!(family("linear").count(ENUMERABLE), Some(322_560));
        // Turning: the classics, every block becoming a turn or mirror of itself.
        let turning = family("turning");
        assert_eq!(turning.count(ENUMERABLE), Some(27_648));
        for id in ["single-rotation", "rotations", "double-rotation", "bbm", "bounce-gas", "hpp-gas", "string-thing"] {
            assert!(turning.holds(&preset(id)), "{id}");
        }
        assert!(!turning.holds(&preset("critters")) && !turning.holds(&preset("tron")));
        // Sparse: the rules that change few blocks; Single rotation changes four, Tron two.
        assert_eq!(family("sparse=2").count(ENUMERABLE), Some(1 + 120));
        assert_eq!(family("sparse=4").count(ENUMERABLE), Some(17_621));
        assert!(family("sparse=4").holds(&preset("single-rotation")));
        assert!(!family("sparse=3").holds(&preset("single-rotation")));
        // Involutions, complements, a stable vacuum.
        assert!(family("involution").holds(&preset("bbm")) && !family("involution").holds(&preset("single-rotation")));
        assert_eq!(family("quarter-turn+involution").count(ENUMERABLE), Some(128));
        assert_eq!(family("quarter-turn+complement").count(ENUMERABLE), Some(128));
        // Cells may be kept relative to a vacuum that is not empty: only a third of these
        // tables leave the empty world empty. Some of the others make the same worlds over a
        // vacuum that flickers, and some make worlds of their own, over a vacuum of stripes.
        assert_eq!(family("half-turn+conserving+complement").count(ENUMERABLE), Some(384));
        assert_eq!(family("half-turn+conserving+complement+stable-vacuum").count(ENUMERABLE), Some(128));
        let (all, stable) = (
            distinct(family("half-turn+conserving+complement").rules()),
            distinct(family("half-turn+conserving+complement+stable-vacuum").rules()),
        );
        assert!(stable.iter().all(|rule| all.contains(rule)) && all.len() > stable.len());
        assert!(all.iter().any(|rule| rule.vacuum_cycle() == [0, 6]));
        assert_eq!(family("quarter-turn+stable-vacuum").count(ENUMERABLE), Some(768));
        // Parity is kept by every conserving rule, and by many more.
        assert!(family("parity").holds(&preset("critters")));
        assert_eq!(family("parity+quarter-turn").count(ENUMERABLE), Some(512));
    }

    #[test]
    fn a_family_is_read_from_names_and_random_requires_nothing() {
        assert_eq!(
            family("mirror+conserving").constraints(),
            [Constraint::Symmetric(Turn::Mirror), Constraint::Conserving]
        );
        assert_eq!(family("random"), Family::default());
        assert_eq!(family("random+mirror"), family("mirror"));
        assert_eq!(family("mirror+mirror"), family("mirror"));
        assert_eq!(
            family(" sparse=5 + weights=1,2,4,1 ").constraints(),
            [Constraint::Sparse(5), Constraint::Weighted(Some([1, 2, 4, 1]))]
        );
        assert!(
            Family::parse("mirrored").is_err()
                && Family::parse("sparse=17").is_err()
                && Family::parse("weights=1,2").is_err()
        );
        for constraint in NAMES.map(|(_, constraint, _)| constraint) {
            assert_eq!(constraint.to_string().parse::<Constraint>(), Ok(constraint));
        }
        assert_eq!(Constraint::Weighted(Some([1, 2, 4, 1])).to_string(), "weights=1,2,4,1");
        // What the help lists can be typed as it stands, with numbers for the letters.
        for (name, _) in catalogue() {
            let typed = name.replace("A,B,C,D", "1,2,4,1").replace('N', "5");
            assert!(Family::parse(&typed).is_ok(), "{name}");
        }
        assert_eq!(catalogue().len(), NAMES.len() + 2);
        // Every rule there is: not to be counted, only sampled.
        assert_eq!(Family::default().count(ENUMERABLE), None);
        assert_eq!(Family::default().sample(5, 1), Family::default().sample(5, 1));
        assert_eq!(Family::default().sample(5, 1).len(), 5);
    }

    #[test]
    fn a_sample_is_of_the_family() {
        let family = family("diagonal+parity");
        let sample = family.sample(50, 7);
        assert_eq!(sample.len(), 50);
        assert!(sample.iter().all(|rule| family.holds(rule)));
        let unique: HashSet<&BlockRule> = sample.iter().collect();
        assert!(unique.len() > 40, "{} different rules in 50", unique.len());
        // The weightings are drawn along with the rules.
        let weighted = Family::parse("weighted+diagonal").unwrap().sample(10, 3);
        assert_eq!(weighted.len(), 10);
        assert!(weighted.iter().all(|rule| matches!(rule.population(), Population::Weighted(_))));
        // No rule keeps a weight of its own and looks the same after a half turn: a sample
        // of nothing is given up on.
        assert_eq!(Family::parse("weighted+half-turn").unwrap().count(ENUMERABLE), Some(0));
        assert!(Family::parse("weighted+half-turn").unwrap().sample(10, 3).is_empty());
    }
}
