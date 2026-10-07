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
//! sampled ([`Family::sample`]). How many rules a family has, and how many canonical rules, is
//! counted on several threads ([`Family::size`]), or known without going through it.

use std::{
    collections::HashSet,
    fmt,
    str::FromStr,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use crate::{
    rules::{
        BlockRule, Population, TURNS_AND_MIRRORS, anti_transpose, complement, flip, itself, keeps_weight, mirror,
        popcount, rotate_180, rotate_cw, transpose, weigh, weightings,
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
    /// Exchanging dead and alive and turning or mirroring the plane together turns every run
    /// into another run: by this turn or mirror, or by some turn or mirror given none. The
    /// rule looks the same with the two states exchanged after a turn or mirror, though it
    /// may not as they are, nor after that turn or mirror alone.
    ComplementTurned(Option<Turn>),
    /// Run backwards, the rule is itself seen through a turn or mirror, or as it is, with
    /// dead and alive exchanged or not: its inverse is the rule seen so. As it is and without
    /// the exchange, the rule is its own inverse, an involution.
    Inverse { through: Option<Turn>, complemented: bool },
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
const NAMES: [(&str, Constraint, &str); 17] = [
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
    (
        "complement=turned",
        Constraint::ComplementTurned(None),
        "The rules for which dead and alive are interchangeable with some turn or mirror, as they may not be as they \
         are",
    ),
    ("involution", Constraint::INVOLUTION, "The rules that are their own inverse"),
    ("stable-vacuum", Constraint::StableVacuum, "The rules that leave the empty world empty"),
    ("turning", Constraint::Turning, "The 27 648 rules that make every block a turn or a mirror of itself"),
    ("sparse", Constraint::Sparse(4), "The rules that change at most N of the 16 blocks; 4 unless said"),
    ("linear", Constraint::Linear, "The 322 560 rules under which patterns superpose"),
];

/// The constraints that go by a name alone, each with its name: `sparse` is the one that
/// allows four blocks to change.
pub fn named() -> impl Iterator<Item = (&'static str, Constraint)> {
    NAMES.iter().map(|&(name, constraint, _)| (name, constraint))
}

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
        if constraint == Constraint::Complement {
            catalogue.push((
                "complement=mirror",
                "The rules for which dead and alive are interchangeable with that mirror, left to right; or quarter-turn, \
                 half-turn, flip, diagonal, anti-diagonal",
            ));
        }
        if constraint == Constraint::INVOLUTION {
            catalogue.push((
                "inverse=mirror",
                "The rules that run backwards as themselves seen in a mirror, left to right: Single rotation is one; \
                 or flip, diagonal, anti-diagonal, half-turn, quarter-turn",
            ));
            catalogue.push((
                "inverse=complemented",
                "The rules that run backwards as themselves with dead and alive exchanged: Critters is one",
            ));
            catalogue.push((
                "inverse=mirror,complemented",
                "The rules that run backwards as themselves seen in a mirror and with dead and alive exchanged; and \
                 so with any turn or mirror",
            ));
        }
    }
    catalogue.push(("random", "Every rule there is: nothing is required"));
    catalogue
}

impl Constraint {
    /// The rules that are their own inverse.
    pub const INVOLUTION: Constraint = Constraint::Inverse { through: None, complemented: false };

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
            Constraint::ComplementTurned(Some(turn)) => rule.looks_the_same_through(turn.transform(), true),
            Constraint::ComplementTurned(None) => {
                TURNS_AND_MIRRORS.iter().any(|&turn| rule.looks_the_same_through(turn, true))
            }
            Constraint::Inverse { through, complemented } => {
                rule.inverse_through(through.map_or(itself as fn(u8) -> u8, Turn::transform), complemented)
            }
            Constraint::StableVacuum => table[0] == 0,
            Constraint::Turning => (0..16).all(|block| orbit(block as u8) >> table[block] & 1 == 1),
            Constraint::Sparse(most) => (0..16).filter(|&block| table[block] != block as u8).count() <= most as usize,
            Constraint::Linear => (0..16).all(|a| (0..16).all(|b| table[a ^ b] == table[a] ^ table[b] ^ table[0])),
        }
    }
}

/// The name a turn or mirror goes by, as a symmetry is asked for.
fn turn_name(turn: Turn) -> &'static str {
    let (name, ..) =
        NAMES.iter().find(|(_, known, _)| *known == Constraint::Symmetric(turn)).expect("every turn has a name");
    name
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Constraint::Weighted(Some(w)) => write!(f, "weights={},{},{},{}", w[0], w[1], w[2], w[3]),
            Constraint::Sparse(most) => write!(f, "sparse={most}"),
            Constraint::Inverse { through, complemented } if through.is_some() || complemented => {
                let parts = through.map(turn_name).into_iter().chain(complemented.then_some("complemented"));
                write!(f, "inverse={}", parts.collect::<Vec<_>>().join(","))
            }
            Constraint::ComplementTurned(Some(turn)) => write!(f, "complement={}", turn_name(turn)),
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
                "complement" => match value.trim().parse::<Constraint>() {
                    Ok(Constraint::Symmetric(turn)) => Ok(Constraint::ComplementTurned(Some(turn))),
                    _ if value.trim() == "turned" => Ok(Constraint::ComplementTurned(None)),
                    _ => Err(format!(
                        "complement={value:?}: with which turn or mirror dead and alive are interchangeable, one of \
                         quarter-turn, half-turn, mirror, flip, diagonal and anti-diagonal, or turned for any"
                    )),
                },
                "inverse" => {
                    // A turn or mirror, `complemented`, or a turn or mirror and `complemented`.
                    let (mut through, mut complemented) = (None, false);
                    for part in value.split(',').map(str::trim) {
                        match part.parse::<Constraint>() {
                            Ok(Constraint::Symmetric(turn)) if through.is_none() => through = Some(turn),
                            _ if part == "complemented" && !complemented => complemented = true,
                            _ => {
                                return Err(format!(
                                    "inverse={value:?}: how the rule run backwards is the rule: seen through one of \
                                     quarter-turn, half-turn, mirror, flip, diagonal and anti-diagonal, or \
                                     complemented, or both, as inverse=mirror,complemented"
                                ));
                            }
                        }
                    }
                    Ok(Constraint::Inverse { through, complemented })
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

/// So many rules a family may have and still be listed whole ([`Family::rules`]).
pub const ENUMERABLE: usize = 8_000_000;

/// So many rules a family may have and still be gone through to count them
/// ([`Family::size`]): under a minute or so on a dozen threads.
pub const COUNTABLE: u64 = 250_000_000;

/// Every rule there is, 16!, and the canonical rules among them. Worked out once: a rule
/// whose world has L tables has 8·L rules in its world, but for some with symmetries of their
/// own, which were gone through one by one; the test `every_rule_there_is_makes_so_many_worlds`
/// does it again, in about half an hour.
pub const EVERY_RULE: Size = Size { rules: 20_922_789_888_000, canonical: Some(552_613_396_971) };

/// How far going through a family has got, as whoever waits for it on another thread sees
/// it, and a way to say that this is far enough.
#[derive(Debug, Default)]
pub struct Progress {
    rules: AtomicU64,
    stopped: AtomicBool,
}

impl Progress {
    /// How many rules have been gone through so far.
    pub fn rules(&self) -> u64 {
        self.rules.load(Ordering::Relaxed)
    }

    /// Far enough: nothing more is gone through, and nothing is said of the family.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

/// How many rules a family has, and how many canonical rules: the worlds its rules make, each
/// once ([`BlockRule::canonical`]). Of a family whose rules are known by a formula, the
/// canonical rules may be more than can be counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub rules: u64,
    pub canonical: Option<u64>,
}
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
        let mut count = 0;
        for (family, counts) in self.each_choice() {
            let whole = Filler::new(&family.constraints, None).fill(&mut |rule| {
                count += counts(&rule) as usize;
                count < cap
            });
            if !whole {
                return None;
            }
        }
        Some(count)
    }

    /// How many rules the family has, and how many canonical rules, as far as `threads`
    /// threads find out with at most `most` rules gone through in all; `progress` is told how
    /// many of the family's own rules they have got through, and can stop them. None if there
    /// are more and nothing else says how many, or when told to stop.
    ///
    /// Every rule there is, and the rules that move at most so many blocks, are known without
    /// going through them. The canonical rules of a family whose rules leave the empty world
    /// empty, and which the turns and mirrors keep, are the orbits of its rules under those
    /// eight: they are counted from how many rules each of the eight leaves alone, which are
    /// the rules of a family with a symmetry more, and far fewer (Burnside's lemma). Of any
    /// other family, the rules are counted that come first, in table order, of the rules of
    /// their world that the family has.
    pub fn size(&self, threads: usize, most: u64, progress: &Progress) -> Option<Size> {
        let family = self.essential();
        if family.constraints.is_empty() {
            return Some(EVERY_RULE);
        }
        let known = family.known_rules();
        let mut budget = most;
        if family.constraints.contains(&Constraint::StableVacuum) && family.kept_by_turns() {
            let rules = match known {
                Some(rules) => rules,
                None => family.go(threads, &mut budget, progress, true, &|_| {})?,
            };
            // Each quarter turn and its opposite leave the same rules alone. The families
            // with a symmetry more are gone through unreported: they are not the rules asked
            // about.
            let mut alone = rules;
            for (turn, times) in [(Turn::Quarter, 2), (Turn::Half, 1), (Turn::Mirror, 1), (Turn::Flip, 1)]
                .into_iter()
                .chain([(Turn::Diagonal, 1), (Turn::AntiDiagonal, 1)])
            {
                let symmetric = Family::new(family.constraints.iter().copied().chain([Constraint::Symmetric(turn)]));
                alone += times * symmetric.go(threads, &mut budget, progress, false, &|_| {})?;
            }
            debug_assert_eq!(alone % 8, 0, "{self:?}");
            return Some(Size { rules, canonical: Some(alone / 8) });
        }
        if let Some(rules) = known.filter(|rules| *rules > most) {
            return Some(Size { rules, canonical: None });
        }
        let canonical = AtomicU64::new(0);
        let first = |rule: &BlockRule| {
            if family.first_of_its_world(rule) {
                canonical.fetch_add(1, Ordering::Relaxed);
            }
        };
        let rules = family.go(threads, &mut budget, progress, true, &first)?;
        Some(Size { rules, canonical: Some(canonical.into_inner()) })
    }

    /// The canonical rules of the family, in order, and how many rules it has, as
    /// [`Family::size`] goes through them: None if there are more than `most` rules, or when
    /// told to stop.
    pub fn canonical_rules(&self, threads: usize, most: u64, progress: &Progress) -> Option<(u64, Vec<BlockRule>)> {
        let family = self.essential();
        if family.known_rules().is_some_and(|rules| rules > most) {
            return None;
        }
        let worlds = Mutex::new(Vec::new());
        let first = |rule: &BlockRule| {
            if family.first_of_its_world(rule) {
                worlds.lock().expect("no thread panicked while holding it").push(rule.canonical());
            }
        };
        let rules = family.go(threads, &mut { most }, progress, true, &first)?;
        let mut worlds = worlds.into_inner().expect("no thread panicked while holding it");
        worlds.sort_unstable_by(|a, b| a.table().cmp(b.table()));
        Some((rules, worlds))
    }

    /// The family without what every rule has anyway: a rule may move all sixteen blocks, and
    /// fifteen where the empty one stays.
    fn essential(&self) -> Family {
        let blocks = if self.constraints.contains(&Constraint::StableVacuum) { 15 } else { 16 };
        Family::new(self.constraints.iter().copied().filter(|c| !matches!(c, Constraint::Sparse(n) if *n >= blocks)))
    }

    /// How many rules the family has, where a formula says so: any rule is one of 16!, one
    /// that leaves the empty block alone one of 15!, and one that moves only so many blocks
    /// is so many of them deranged.
    fn known_rules(&self) -> Option<u64> {
        match self.constraints[..] {
            [] => Some(moving_at_most(16, 16)),
            [Constraint::StableVacuum] => Some(moving_at_most(15, 15)),
            [Constraint::Sparse(most)] => Some(moving_at_most(16, most as u64)),
            [Constraint::StableVacuum, Constraint::Sparse(most)]
            | [Constraint::Sparse(most), Constraint::StableVacuum] => Some(moving_at_most(15, most as u64)),
            _ => None,
        }
    }

    /// Whether a rule of the family turned or mirrored is a rule of the family: a mirror
    /// across one axis is another after a quarter turn, and a weight of the corners another
    /// weight.
    fn kept_by_turns(&self) -> bool {
        // A rule that runs backwards as itself turned by a quarter turn one way runs backwards
        // as itself turned the other way as well, and a half turn is the same from any side.
        let any_side = |turn: &Turn| matches!(turn, Turn::Quarter | Turn::Half);
        self.constraints.iter().all(|constraint| match constraint {
            Constraint::Symmetric(turn) => any_side(turn),
            Constraint::Inverse { through, .. } => through.as_ref().is_none_or(any_side),
            Constraint::ComplementTurned(turn) => turn.as_ref().is_none_or(any_side),
            Constraint::Weighted(_) => false,
            _ => true,
        })
    }

    /// Is the rule the first, in table order, of the rules of its world that the family has?
    /// Counting those counts the worlds of the family, each once.
    fn first_of_its_world(&self, rule: &BlockRule) -> bool {
        let own = rule.table();
        rule.each_in_world(|table| {
            table >= own || !self.holds(&BlockRule::new(*table).expect("a rule of the same world is a rule"))
        })
    }

    /// Goes through the family's rules on `threads` threads, handing each to `found` on the
    /// thread that finds it, and takes the rules gone through off `budget`. Returns how many
    /// there are, or None once there are more than the budget allows or when `progress` is
    /// told to stop; `report` says whether `progress` is told how far this has got.
    fn go(
        &self,
        threads: usize,
        budget: &mut u64,
        progress: &Progress,
        report: bool,
        found: &(dyn Fn(&BlockRule) + Sync),
    ) -> Option<u64> {
        let mut total = 0;
        for (family, counts) in self.each_choice() {
            total += family.go_through(threads, budget, progress, report, &*counts, found)?;
        }
        Some(total)
    }

    /// The family as families the filler can go through: itself, unless a constraint stands
    /// for any of several, which the filler cannot force; then the family with each of those
    /// in its place, and with each whether a rule gone through is to count: a rule that has
    /// the property is counted under the first of the several it has, and so once.
    fn each_choice(&self) -> Vec<(Family, Counts)> {
        let Some((place, choices)) = choices(&self.constraints) else {
            return vec![(self.clone(), Box::new(|_| true))];
        };
        let any = self.constraints[place];
        let mut families = Vec::new();
        for (which, &choice) in choices.iter().enumerate() {
            let mut constraints = self.constraints.clone();
            constraints[place] = choice;
            for (family, counts) in Family::new(constraints).each_choice() {
                let before = choices[..which].to_vec();
                let counts = move |rule: &BlockRule| {
                    any.holds(rule) && !before.iter().any(|first| first.holds(rule)) && counts(rule)
                };
                families.push((family, Box::new(counts) as Counts));
            }
        }
        families
    }

    /// As [`Family::go`], handing over and counting only the rules that `wanted` says, though
    /// every rule gone through is taken off the budget.
    fn go_through(
        &self,
        threads: usize,
        budget: &mut u64,
        progress: &Progress,
        report: bool,
        wanted: &(dyn Fn(&BlockRule) -> bool + Sync),
        found: &(dyn Fn(&BlockRule) + Sync),
    ) -> Option<u64> {
        let most = *budget;
        // The tables begun in as many ways as the threads can share out among themselves.
        let starts = Filler::starts(&self.constraints, 32 * threads.max(1));
        let next = AtomicUsize::new(0);
        let (total, gone, over) = (AtomicU64::new(0), AtomicU64::new(0), AtomicBool::new(false));
        // Counts are added up now and then, not at every rule: so many threads would queue
        // for the same counter.
        let add = |counted: &mut u64, through: &mut u64| {
            total.fetch_add(*counted, Ordering::Relaxed);
            let all = gone.fetch_add(*through, Ordering::Relaxed) + *through;
            if report {
                progress.rules.fetch_add(*through, Ordering::Relaxed);
            }
            (*counted, *through) = (0, 0);
            if all > most {
                over.store(true, Ordering::Relaxed);
            }
        };
        let done = || over.load(Ordering::Relaxed) || progress.stopped();
        std::thread::scope(|scope| {
            for _ in 0..threads.max(1) {
                scope.spawn(|| {
                    let (mut counted, mut through) = (0, 0);
                    while !done() {
                        let Some(start) = starts.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        let mut filler = Filler::new(&self.constraints, None);
                        filler.stop = Some(&done);
                        if !filler.start(start) {
                            continue;
                        }
                        filler.fill(&mut |rule| {
                            if wanted(&rule) {
                                found(&rule);
                                counted += 1;
                            }
                            through += 1;
                            if through == 4096 {
                                add(&mut counted, &mut through);
                                return !done();
                            }
                            true
                        });
                    }
                    add(&mut counted, &mut through);
                });
            }
        });
        if done() {
            return None;
        }
        *budget -= gone.into_inner();
        Some(total.into_inner())
    }

    /// Every rule of the family, for a family whose [`Family::count`] fits.
    pub fn rules(&self) -> Vec<BlockRule> {
        if self.constraints.is_empty() {
            return Vec::new();
        }
        let mut rules = Vec::new();
        for (family, counts) in self.each_choice() {
            Filler::new(&family.constraints, None).fill(&mut |rule| {
                if counts(&rule) {
                    rules.push(rule);
                }
                true
            });
        }
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
    /// be empty, and where any of several will do, one is drawn first that may have no rule
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
        let mut any = Vec::new();
        while let Some((place, choices)) = choices(&constraints) {
            any.push(constraints[place]);
            constraints[place] = choices[(rng.next_u64() % choices.len() as u64) as usize];
        }
        let mut found = None;
        Filler::new(&constraints, Some(rng)).fill(&mut |rule| {
            found = Some(rule);
            false
        });
        found.filter(|rule| any.iter().all(|any| any.holds(rule)))
    }
}

/// Whether a rule gone through is to count, where a constraint stands for any of several.
type Counts = Box<dyn Fn(&BlockRule) -> bool + Sync>;

/// A constraint that stands for any of several, which the filler cannot force: any weighting
/// but the plain one, or any turn or mirror with the exchange of the two states. The place of
/// the first such among `constraints`, and the constraints it stands for.
fn choices(constraints: &[Constraint]) -> Option<(usize, Vec<Constraint>)> {
    let any =
        |constraint: &Constraint| matches!(constraint, Constraint::Weighted(None) | Constraint::ComplementTurned(None));
    let place = constraints.iter().position(any)?;
    let choices = match constraints[place] {
        Constraint::Weighted(None) => weightings()
            .into_iter()
            .filter(|weights| *weights != [1; 4])
            .map(|weights| Constraint::Weighted(Some(weights)))
            .collect(),
        _ => [Turn::Quarter, Turn::Half, Turn::Mirror, Turn::Flip, Turn::Diagonal, Turn::AntiDiagonal]
            .into_iter()
            .map(|turn| Constraint::ComplementTurned(Some(turn)))
            .collect(),
    };
    Some((place, choices))
}

/// How many ways there are to rearrange so many blocks so that none stays where it was: one
/// way for none, none for one, and (n − 1)·(D(n − 1) + D(n − 2)) for n.
fn derangements(blocks: u64) -> u64 {
    let (mut two_before, mut before) = (1, 0);
    if blocks == 0 {
        return 1;
    }
    for n in 2..=blocks {
        (two_before, before) = (before, (n - 1) * (two_before + before));
    }
    before
}

/// How many rules move at most `most` of so many blocks and leave the others alone.
fn moving_at_most(blocks: u64, most: u64) -> u64 {
    let choose = |k: u64| (0..k).fold(1, |ways, i| ways * (blocks - i) / (i + 1));
    (0..=most.min(blocks)).map(|moved| choose(moved) * derangements(moved)).sum()
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
    /// Says when to give up, if anyone does.
    stop: Option<&'a (dyn Fn() -> bool + Sync)>,
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
            stop: None,
        }
    }

    /// Ways to begin filling in the table, as lists of outcomes for the blocks [`Filler::next`]
    /// names in turn: as few as there are at least `wanted` of, as far as a few blocks deep.
    fn starts(constraints: &[Constraint], wanted: usize) -> Vec<Vec<u8>> {
        let mut starts = vec![Vec::new()];
        for _ in 0..4 {
            if starts.len() >= wanted {
                break;
            }
            let mut longer = Vec::new();
            for start in &starts {
                let mut filler = Filler::new(constraints, None);
                if !filler.start(start) {
                    continue;
                }
                let Some(block) = filler.next() else {
                    // A whole table already.
                    longer.push(start.clone());
                    continue;
                };
                for outcome in 0..16u8 {
                    let mark = filler.trail.len();
                    if filler.used >> outcome & 1 == 0 && filler.assign(block, outcome) {
                        longer.push(start.iter().copied().chain([outcome]).collect());
                    }
                    filler.undo(mark);
                }
            }
            starts = longer;
        }
        starts
    }

    /// Fills in the first blocks as a list of their outcomes says. False if the constraints
    /// forbid it.
    fn start(&mut self, outcomes: &[u8]) -> bool {
        outcomes.iter().all(|&outcome| self.next().is_some_and(|block| self.assign(block, outcome)))
    }

    /// Goes through every way to finish the table, handing each rule to `found` until it
    /// says to stop. Returns whether it got to the end.
    fn fill(&mut self, found: &mut dyn FnMut(BlockRule) -> bool) -> bool {
        if self.stop.is_some_and(|stop| stop()) {
            return false;
        }
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
    /// blocks tied to it by a symmetry, its complement's, the one the inverse ties to it (of
    /// an involution, its own), the sums of a linear rule. Returns false if the constraints
    /// forbid it.
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
                    Constraint::ComplementTurned(Some(turn)) => {
                        let transform = turn.transform();
                        pending.push((complement(transform(block)), complement(transform(outcome))));
                    }
                    // The inverse is the rule seen through `see`: the block the outcome is
                    // seen as goes to the block seen so.
                    Constraint::Inverse { through, complemented } => {
                        let transform = through.map_or(itself as fn(u8) -> u8, Turn::transform);
                        let see = |block| if complemented { complement(transform(block)) } else { transform(block) };
                        pending.push((see(outcome), see(block)));
                    }
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
mod every_rule;

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
        // How a rule runs backwards: Single rotation as its mirror image, Critters with dead
        // and alive exchanged, the HPP gas as itself. A rule that runs backwards as itself
        // seen in a mirror is one whose table followed by the mirror is an involution, so
        // there are as many such rules as involutions; so for the half turn, so complemented.
        let single_rotation = preset("single-rotation");
        for name in ["inverse=mirror", "inverse=flip", "inverse=diagonal", "inverse=anti-diagonal"] {
            assert!(family(name).holds(&single_rotation), "{name}");
        }
        assert!(
            !family("inverse=half-turn").holds(&single_rotation)
                && !family("inverse=complemented").holds(&single_rotation)
        );
        assert!(
            family("inverse=complemented").holds(&preset("critters"))
                && !family("involution").holds(&preset("critters"))
        );
        assert!(family("involution").holds(&preset("hpp-gas")) && family("inverse=mirror").holds(&preset("hpp-gas")));
        // Of the rules that look the same after a quarter turn, as many run backwards as
        // themselves mirrored as with dead and alive exchanged as well; and the ones that run
        // backwards as themselves turned are the involutions, since turned they are themselves.
        assert_eq!(family("quarter-turn+inverse=mirror").count(ENUMERABLE), Some(448));
        assert_eq!(family("quarter-turn+inverse=mirror,complemented").count(ENUMERABLE), Some(448));
        assert_eq!(family("quarter-turn+inverse=mirror+involution").count(ENUMERABLE), Some(48));
        assert_eq!(family("quarter-turn+inverse=complemented").count(ENUMERABLE), Some(128));
        let sorted = |mut rules: Vec<BlockRule>| {
            rules.sort_unstable_by(|a, b| a.table().cmp(b.table()));
            rules
        };
        for name in ["quarter-turn+inverse=half-turn", "quarter-turn+inverse=quarter-turn"] {
            assert_eq!(sorted(family(name).rules()), sorted(family("quarter-turn+involution").rules()), "{name}");
        }
        // A rule that runs backwards as itself seen in a mirror is one whose table followed
        // by the mirror is an involution; with dead and alive exchanged as well, followed by
        // both. So there are as many of them as there are involutions.
        let followed_by = |rule: &BlockRule, see: &dyn Fn(u8) -> u8| {
            BlockRule::new(std::array::from_fn(|block| rule.table()[see(block as u8) as usize])).unwrap()
        };
        for rule in family("quarter-turn+inverse=mirror").rules() {
            assert!(rule.inverse_through(mirror, false) && rule.commutes_with(rotate_cw), "{rule}");
            assert!(Constraint::INVOLUTION.holds(&followed_by(&rule, &mirror)), "{rule}");
        }
        for rule in family("half-turn+inverse=mirror,complemented").sample(30, 3) {
            assert!(rule.inverse_through(mirror, true), "{rule}");
            assert!(Constraint::INVOLUTION.holds(&followed_by(&rule, &|block| complement(mirror(block)))), "{rule}");
        }
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
    fn a_family_is_counted_on_several_threads() {
        let size = |name: &str| family(name).size(4, u64::MAX, &Progress::default());
        let exactly = |rules, canonical| Some(Size { rules, canonical: Some(canonical) });
        // As listing the rules and their canonical forms has it.
        for (name, rules, canonical) in [
            ("quarter-turn", 1536, 584),
            ("momentum", 228, 41),
            ("turning", 27_648, 3_808),
            ("linear", 322_560, 2_606),
            ("weighted", 216_480, 20_729),
            ("conserving", 845_040, 79_612),
            ("half-turn", 1_105_920, 146_040),
            ("sparse=4", 17_621, 2_351),
        ] {
            assert_eq!(size(name), exactly(rules, canonical), "{name}");
        }
        let progress = Progress::default();
        let (rules, worlds) = family("quarter-turn").canonical_rules(4, u64::MAX, &progress).unwrap();
        let mut listed = distinct(family("quarter-turn").rules());
        listed.sort_unstable_by(|a, b| a.table().cmp(b.table()));
        assert_eq!((rules, worlds, progress.rules()), (1536, listed, 1536));
        // Where the empty world stays empty, by Burnside, as counting them one by one has it.
        for name in [
            "stable-vacuum+half-turn",
            "stable-vacuum+quarter-turn",
            "stable-vacuum+conserving",
            "stable-vacuum+linear",
            "stable-vacuum+momentum",
            "stable-vacuum+involution+sparse=6",
        ] {
            let (rules, worlds) = family(name).canonical_rules(4, u64::MAX, &Progress::default()).unwrap();
            assert_eq!(size(name), exactly(rules, worlds.len() as u64), "{name}");
        }
        assert_eq!(size("stable-vacuum+half-turn"), exactly(276_480, 69_760));
        // What is said to have been gone through is the family's own rules, not the smaller
        // families gone through for its symmetry; those count towards how many may be gone
        // through in all.
        let progress = Progress::default();
        assert_eq!(family("stable-vacuum+conserving").size(4, u64::MAX, &progress), exactly(414_720, 52_320));
        assert_eq!(progress.rules(), 414_720);
        assert_eq!(family("stable-vacuum+conserving").size(4, 414_720, &Progress::default()), None);
        assert!(family("stable-vacuum+conserving").size(4, 418_560, &Progress::default()).is_some());
        // Too many to go through, and known all the same.
        assert_eq!(size("random"), Some(EVERY_RULE));
        assert_eq!(size("sparse=16"), Some(EVERY_RULE));
        assert_eq!(size("stable-vacuum"), exactly((1..=15).product(), 163_459_883_712));
        assert_eq!(size("stable-vacuum+sparse=15"), size("stable-vacuum"));
        let sparse = family("sparse=10").size(4, 1_000_000, &Progress::default());
        assert_eq!(sparse, Some(Size { rules: 12_432_004_331, canonical: None }));
        // Neither known nor gone through: more than wanted, or stopped.
        assert_eq!(family("half-turn").size(4, 1000, &Progress::default()), None);
        let stopped = Progress::default();
        stopped.stop();
        assert_eq!(family("half-turn").size(4, u64::MAX, &stopped), None);
        assert_eq!(family("half-turn").canonical_rules(4, 1000, &Progress::default()), None);
    }

    #[test]
    fn the_formulas_count_as_the_filler_does() {
        for most in 2..=6 {
            let sparse = family(&format!("sparse={most}"));
            assert_eq!(sparse.known_rules(), sparse.count(ENUMERABLE).map(|count| count as u64), "sparse={most}");
            let stable = family(&format!("stable-vacuum+sparse={most}"));
            assert_eq!(stable.known_rules(), stable.count(ENUMERABLE).map(|count| count as u64), "sparse={most}");
        }
        assert_eq!(family("sparse=7").known_rules(), Some(23_541_693));
        assert_eq!(family("random").known_rules(), Some((1..=16).product()));
    }

    #[test]
    fn the_turns_and_mirrors_keep_what_they_are_said_to_keep() {
        for name in [
            "quarter-turn",
            "half-turn",
            "conserving",
            "parity",
            "momentum",
            "turning",
            "linear",
            "involution",
            "complement",
            "stable-vacuum",
            "sparse=6",
            "inverse=complemented",
            "inverse=half-turn",
            "inverse=quarter-turn",
            "inverse=quarter-turn,complemented",
            "complement=turned",
            "complement=half-turn",
            "complement=quarter-turn",
        ] {
            let family = family(name);
            assert!(family.kept_by_turns(), "{name}");
            for rule in family.sample(40, 5) {
                for turn in TURNS_AND_MIRRORS {
                    assert!(family.holds(&rule.seen_through(turn)), "{name}: {rule}");
                }
            }
        }
        assert!(!family("mirror").kept_by_turns() && !family("weighted").kept_by_turns());
        assert!(!family("inverse=mirror").kept_by_turns() && !family("inverse=diagonal,complemented").kept_by_turns());
        assert!(!family("complement=mirror").kept_by_turns());
        // Seen through a quarter turn, a rule of the mirror's family is one of the flip's.
        let through_the_mirror = family("inverse=mirror");
        for rule in through_the_mirror.sample(20, 9) {
            assert!(family("inverse=flip").holds(&rule.seen_through(rotate_cw)), "{rule}");
        }
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
        // How the rule runs backwards: a turn or mirror, complemented, or both, in any order.
        let through = |through, complemented| Constraint::Inverse { through, complemented };
        assert_eq!("involution".parse::<Constraint>(), Ok(Constraint::INVOLUTION));
        assert_eq!(Constraint::INVOLUTION.to_string(), "involution");
        assert_eq!("inverse=mirror".parse::<Constraint>(), Ok(through(Some(Turn::Mirror), false)));
        assert_eq!("inverse=complemented".parse::<Constraint>(), Ok(through(None, true)));
        assert_eq!("inverse=complemented, half-turn".parse::<Constraint>(), Ok(through(Some(Turn::Half), true)));
        assert_eq!(through(Some(Turn::Half), true).to_string(), "inverse=half-turn,complemented");
        assert_eq!(through(Some(Turn::AntiDiagonal), false).to_string(), "inverse=anti-diagonal");
        assert_eq!(through(None, true).to_string(), "inverse=complemented");
        for wrong in ["inverse=", "inverse=same", "inverse=turned", "inverse=mirror,flip", "inverse=conserving"] {
            assert!(wrong.parse::<Constraint>().is_err(), "{wrong}");
        }
        // Dead and alive interchangeable with a turn or mirror: one of them, or any.
        assert_eq!("complement=mirror".parse::<Constraint>(), Ok(Constraint::ComplementTurned(Some(Turn::Mirror))));
        assert_eq!("complement=turned".parse::<Constraint>(), Ok(Constraint::ComplementTurned(None)));
        assert_eq!(Constraint::ComplementTurned(Some(Turn::Diagonal)).to_string(), "complement=diagonal");
        assert_eq!(Constraint::ComplementTurned(None).to_string(), "complement=turned");
        for wrong in ["complement=", "complement=same", "complement=turned,mirror", "complement=linear"] {
            assert!(wrong.parse::<Constraint>().is_err(), "{wrong}");
        }
        // What the help lists can be typed as it stands, with numbers for the letters.
        for (name, _) in catalogue() {
            let typed = name.replace("A,B,C,D", "1,2,4,1").replace('N', "5");
            assert!(Family::parse(&typed).is_ok(), "{name}");
        }
        assert_eq!(catalogue().len(), NAMES.len() + 6);
        // Every rule there is: not to be counted, only sampled.
        assert_eq!(Family::default().count(ENUMERABLE), None);
        assert_eq!(Family::default().sample(5, 1), Family::default().sample(5, 1));
        assert_eq!(Family::default().sample(5, 1).len(), 5);
    }

    #[test]
    fn dead_and_alive_may_be_interchangeable_only_with_a_turn_or_mirror() {
        // This rule looks the same with dead and alive exchanged across a diagonal, and not
        // as they are, nor across the diagonal alone; the identity looks the same every way.
        let hidden: BlockRule = "0,1,2,5,4,10,8,9,6,7,12,11,3,13,14,15".parse().unwrap();
        assert!(family("complement=diagonal").holds(&hidden) && family("complement=turned").holds(&hidden));
        assert!(!family("complement").holds(&hidden) && !family("diagonal").holds(&hidden));
        assert!(!family("complement=mirror").holds(&hidden) && !family("complement=anti-diagonal").holds(&hidden));
        assert!(
            family("complement=turned").holds(&BlockRule::identity())
                && family("complement").holds(&BlockRule::identity())
        );
        // Any turn or mirror: the rules of the six together, each once, as going through them
        // on threads has it too; and a sample is of them.
        let turned = family("quarter-turn+complement=turned");
        let mut union: HashSet<BlockRule> = HashSet::new();
        for name in ["quarter-turn", "half-turn", "mirror", "flip", "diagonal", "anti-diagonal"] {
            union.extend(family(&format!("quarter-turn+complement={name}")).rules());
        }
        let rules = turned.rules();
        assert_eq!((rules.len(), union.len()), (160, 160));
        assert!(rules.iter().all(|rule| union.contains(rule)) && rules.iter().all(|rule| turned.holds(rule)));
        assert_eq!(turned.count(ENUMERABLE), Some(rules.len()));
        let size = turned.size(4, u64::MAX, &Progress::default()).unwrap();
        assert_eq!(size.rules, rules.len() as u64);
        assert_eq!(size.canonical, Some(distinct(rules.clone()).len() as u64));
        assert!(family("complement=turned").sample(20, 2).iter().all(|rule| family("complement=turned").holds(rule)));
        assert!(
            family("complement=turned+half-turn")
                .sample(20, 2)
                .iter()
                .all(|rule| family("complement=turned+half-turn").holds(rule))
        );
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
