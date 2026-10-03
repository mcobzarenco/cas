//! Block rules on the Margolus neighbourhood.
//!
//! The grid is partitioned into 2×2 blocks. On even generations the blocks are aligned with the
//! origin; on odd generations the partition is shifted by one cell diagonally, so every cell sees
//! the other three members of its old block move to three different new blocks.
//!
//! A rule maps each of the 16 possible block states to a new state. Cells are packed into a
//! nibble in row-major order with `y` pointing down, matching the image rows:
//!
//! ```text
//!   bit 0 | bit 1
//!   ------+------
//!   bit 2 | bit 3
//! ```
//!
//! A rule is *reversible* exactly when its table is a permutation of the 16 states; running time
//! backwards then means applying the inverse permutation with the partition the forward step
//! used. Only reversible rules can be constructed here.
//!
//! Nothing says the empty block must stay empty. Then empty space is not all dead cells but a
//! texture that changes with every step, the *vacuum*, and what one wants to look at is how a
//! pattern differs from it. [`BlockRule::vacuum_cycle`] and [`BlockRule::relative_to_vacuum`]
//! describe the vacuum and that difference for any rule.
//!
//! As text, a rule is its table: sixteen comma-separated states, the outcome of state 0 first,
//! e.g. `0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15` for Single Rotation. That is the notation of
//! dmishin's simulator and, with an `MS,D` prefix and `;` separators, of MCell.
//!
//! The rules that look the same after a quarter turn have a second name. They are Morita's
//! *elementary square partitioned cellular automata* seen at 45°, and he numbers them with six
//! hexadecimal digits, as in ESPCA-01c5ef: see [`BlockRule::from_espca`].

use std::{fmt, str::FromStr};

/// A named rule: one from the literature, or one that the search of this project found.
#[derive(Debug)]
pub struct Preset {
    /// Stable identifier: accepted on the command line and used to name UI widgets.
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub table: [u8; 16],
    pub source: Source,
}

/// Where a preset is from. The rule menu lists them source by source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The rule collections of dmishin's simulator and of MCell.
    Collections,
    /// Morita, *Reversible World of Cellular Automata* (2024), where the rules go by their
    /// ESPCA numbers ([`BlockRule::from_espca`]).
    Morita,
    /// Found by going through families of rules ([`crate::search`]), and named here after
    /// what they do.
    Search,
}

impl Preset {
    pub fn rule(&self) -> BlockRule {
        BlockRule::new(self.table).expect("presets are reversible")
    }
}

/// The rules offered in the rule menu. Tables are the published ones (dmishin's simulator and
/// the MCell collection use the same numbering); the tests check each blurb against its table,
/// and what a blurb says of a found rule's doings against the rule at work.
pub static PRESETS: [Preset; 26] = [
    Preset {
        id: "single-rotation",
        name: "Single rotation",
        blurb: "Blocks with exactly one live cell rotate 90° clockwise. Population is \
                conserved and the vacuum is stable.",
        table: [0, 2, 8, 3, 1, 5, 6, 7, 4, 9, 10, 11, 12, 13, 14, 15],
        source: Source::Collections,
    },
    Preset {
        id: "critters",
        name: "Critters",
        blurb: "0, 1 or 4 live cells: invert the block. 2: keep it. 3: invert and rotate 180°. \
                Every empty block fills up, so the vacuum flips on every step.",
        table: [15, 14, 13, 3, 11, 5, 6, 1, 7, 9, 10, 2, 12, 4, 8, 0],
        source: Source::Collections,
    },
    Preset {
        id: "bbm",
        name: "Billiard ball machine",
        blurb: "Margolus' billiard-ball model: a lone cell crosses its block diagonally, two \
                cells on a diagonal bounce to the other diagonal, everything else stays.",
        table: [0, 8, 4, 3, 2, 5, 9, 7, 1, 6, 10, 11, 12, 13, 14, 15],
        source: Source::Collections,
    },
    Preset {
        id: "bounce-gas",
        name: "Bounce gas",
        blurb: "The billiard-ball machine with three-cell blocks turned by 180° as well: a gas \
                of diagonal particles that bounce off each other.",
        table: [0, 8, 4, 3, 2, 5, 9, 14, 1, 6, 10, 13, 12, 11, 7, 15],
        source: Source::Collections,
    },
    Preset {
        id: "hpp-gas",
        name: "HPP gas",
        blurb: "The HPP lattice gas: every block turns by 180°, so particles fly diagonally, \
                except head-on pairs, which scatter onto the other diagonal.",
        table: [0, 8, 4, 12, 2, 10, 9, 14, 1, 6, 5, 13, 3, 11, 7, 15],
        source: Source::Collections,
    },
    Preset {
        id: "tron",
        name: "Tron",
        blurb: "Empty and full blocks swap, every other block stays as it is. The vacuum flips \
                on every step.",
        table: [15, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 0],
        source: Source::Collections,
    },
    Preset {
        id: "rotations",
        name: "Rotations",
        blurb: "One-cell and three-cell blocks rotate 90° clockwise; two-cell blocks jump to \
                the opposite side or the other diagonal.",
        table: [0, 2, 8, 12, 1, 10, 9, 11, 4, 6, 5, 14, 3, 7, 13, 15],
        source: Source::Collections,
    },
    Preset {
        id: "double-rotation",
        name: "Double rotation",
        blurb: "One-cell blocks rotate 90° clockwise and three-cell blocks 90° \
                counter-clockwise; everything else stays.",
        table: [0, 2, 8, 3, 1, 5, 6, 13, 4, 9, 10, 7, 12, 14, 11, 15],
        source: Source::Collections,
    },
    Preset {
        id: "string-thing",
        name: "String thing",
        blurb: "Only two-cell blocks change: adjacent pairs jump to the opposite side, \
                diagonal pairs to the other diagonal.",
        table: [0, 1, 2, 12, 4, 10, 9, 7, 8, 6, 5, 11, 3, 13, 14, 15],
        source: Source::Collections,
    },
    Preset {
        id: "swap-on-diagonal",
        name: "Swap on diagonal",
        blurb: "Every block turns by 180°: each cell swaps with the one diagonally opposite, \
                so particles fly diagonally and never interact.",
        table: [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15],
        source: Source::Collections,
    },
    Preset {
        id: "espca-01c5ef",
        name: "ESPCA-01c5ef",
        blurb: "One-cell blocks rotate 90° counter-clockwise, three-cell blocks clockwise, and \
                two cells on a diagonal jump to the other diagonal. Morita builds reversible \
                Turing machines in it, with a spaceship of period 12 as the signal.",
        table: [0, 4, 1, 3, 8, 5, 9, 11, 2, 6, 10, 14, 12, 7, 13, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-01caef",
        name: "ESPCA-01caef",
        blurb: "One-cell blocks rotate 90° counter-clockwise and three-cell blocks clockwise: \
                the mirror image of Double rotation. Rich in spaceships; Morita builds \
                reversible Turing machines in it.",
        table: [0, 4, 1, 3, 8, 5, 6, 11, 2, 9, 10, 14, 12, 7, 13, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-02c5bf",
        name: "ESPCA-02c5bf",
        blurb: "The billiard-ball machine with three-cell blocks rotated 90° \
                counter-clockwise. Only lone cells travel, yet Morita shows that it can \
                simulate every reversible rule of this family.",
        table: [0, 8, 4, 3, 2, 5, 9, 13, 1, 6, 10, 7, 12, 14, 11, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-016a7f",
        name: "ESPCA-016a7f",
        blurb: "One-cell blocks rotate 90° counter-clockwise, pairs side by side clockwise, \
                three-cell blocks by 180°. Has a spaceship of period 3 and many slow ones.",
        table: [0, 4, 1, 10, 8, 3, 6, 14, 2, 9, 12, 13, 5, 11, 7, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-0945df",
        name: "ESPCA-0945df",
        blurb: "A lone cell gains a neighbour, and two side by side lose it again; two on a \
                diagonal jump to the other diagonal. Cells are not conserved. Two full blocks \
                side by side fire two spaceships every 10 steps.",
        table: [0, 5, 3, 2, 12, 1, 9, 7, 10, 6, 8, 11, 4, 13, 14, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-09457f",
        name: "ESPCA-09457f",
        blurb: "ESPCA-0945df with three-cell blocks turned by 180° as well. A single cell is \
                a gun: it sends out four spaceships every 8 steps, in either direction of \
                time.",
        table: [0, 5, 3, 2, 12, 1, 9, 14, 10, 6, 8, 13, 4, 11, 7, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-098aef",
        name: "ESPCA-098aef",
        blurb: "A lone cell gains a neighbour, and two side by side lose the first of them; \
                three-cell blocks rotate 90° clockwise. Cells are not conserved; there are \
                spaceships of period 10 and 17.",
        table: [0, 5, 3, 1, 12, 4, 6, 11, 10, 9, 2, 14, 8, 7, 13, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-0925bf",
        name: "ESPCA-0925bf",
        blurb: "A lone cell gains a neighbour, two side by side leave one cell on the \
                opposite side; two on a diagonal jump to the other diagonal, three-cell blocks \
                rotate 90° counter-clockwise. A single cell grows into a disk.",
        table: [0, 5, 3, 8, 12, 2, 9, 13, 10, 6, 4, 7, 1, 14, 11, 15],
        source: Source::Morita,
    },
    Preset {
        id: "espca-0dca8f",
        name: "ESPCA-0dca8f",
        blurb: "A lone cell becomes the three cells around the opposite corner, and those \
                three that cell. A single cell grows into shapes that look like fractals.",
        table: [0, 7, 11, 3, 13, 5, 6, 1, 14, 9, 10, 2, 12, 4, 8, 15],
        source: Source::Morita,
    },
    Preset {
        id: "steady-blob",
        name: "Steady blob",
        blurb: "Cells are made and unmade, yet a blob settles at one and a half times its cells \
                and stays there, letting a slow spaceship go now and then.",
        table: [0, 1, 11, 5, 13, 12, 6, 7, 8, 9, 3, 2, 10, 4, 14, 15],
        source: Source::Search,
    },
    Preset {
        id: "creeping-blob",
        name: "Creeping blob",
        blurb: "Like Steady blob, but the blob keeps growing, ever more slowly: twice its cells \
                after 30 000 generations. The richer of the two in slow spaceships.",
        table: [0, 1, 11, 5, 13, 12, 15, 14, 8, 9, 3, 2, 10, 4, 7, 6],
        source: Source::Search,
    },
    Preset {
        id: "ship-factory",
        name: "Ship factory",
        blurb: "A blob stays a blob while it sends out small spaceships by the hundred, at a \
                third of the speed of light: it makes the cells it loses.",
        table: [0, 1, 11, 10, 13, 12, 9, 7, 8, 6, 3, 2, 5, 4, 14, 15],
        source: Source::Search,
    },
    Preset {
        id: "plus-ships",
        name: "Plus ships",
        blurb: "A blob takes on the texture of a maze and throws plus-shaped spaceships along \
                one diagonal. Cells are not conserved, and empty space goes through four states.",
        table: [15, 8, 13, 3, 11, 5, 9, 14, 1, 0, 10, 2, 12, 4, 7, 6],
        source: Source::Search,
    },
    Preset {
        id: "four-way-gun",
        name: "Four-way gun",
        blurb: "A single cell is a gun: four streams of small spaceships leave it along the \
                diagonals. A blob turns to noise. This is ESPCA-f6b580.",
        table: [15, 10, 12, 13, 3, 14, 9, 1, 5, 6, 7, 2, 11, 4, 8, 0],
        source: Source::Search,
    },
    Preset {
        id: "crossing-fleets",
        name: "Crossing fleets",
        blurb: "A blob throws off spaceships in all four diagonal directions, kind after kind, \
                and what stays behind oscillates with many periods. Cells are conserved, \
                relative to a vacuum that flips.",
        table: [15, 7, 13, 10, 14, 3, 6, 1, 11, 9, 12, 4, 5, 8, 2, 0],
        source: Source::Search,
    },
    Preset {
        id: "diagonal-traffic",
        name: "Diagonal traffic",
        blurb: "Spaceships fly both ways along one diagonal, a dozen kinds from one blob. \
                Single rotation with five of the pairs going round in a cycle and two blocks \
                of three swapped; cells are conserved.",
        table: [0, 2, 8, 5, 1, 6, 12, 14, 4, 9, 3, 11, 10, 13, 7, 15],
        source: Source::Search,
    },
];

/// Why a table is not a usable rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleError {
    /// Not exactly 16 entries.
    Length(usize),
    /// An entry is not a block state.
    Entry(String),
    /// Two block states have the same outcome, so a step could not be undone.
    NotReversible { output: u8, inputs: (u8, u8) },
    /// Not the number of an ESPCA.
    Espca(String),
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length(n) => write!(f, "a rule has 16 entries, found {n}"),
            Self::Entry(entry) => write!(f, "{entry:?} is not a block state (0–15)"),
            Self::NotReversible { output, inputs: (a, b) } => write!(
                f,
                "not reversible: blocks {a} and {b} both become {output}"
            ),
            Self::Espca(number) => write!(
                f,
                "{number:?} is not the number of an ESPCA: six hexadecimal digits like 01c5ef, \
                 the first and the last 0 or f, the fourth 0, 5, a or f"
            ),
        }
    }
}

impl std::error::Error for RuleError {}

/// What a rule does to the number of live cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Population {
    /// Every block keeps its number of live cells.
    Conserved,
    /// The cells themselves are not conserved, but the cells that differ from the vacuum are,
    /// as in Critters.
    ConservedRelativeToVacuum,
    /// Not the number of cells but a weighted number of them is conserved (relative to the
    /// vacuum): a cell weighs according to its corner of the block about to be rewritten,
    /// top-left, top-right, bottom-left, bottom-right. Cells are made and unmade, a heavy one
    /// for light ones, yet only within the ratio of the weights ([`BlockRule::conserved_weights`]).
    Weighted([u8; 4]),
    NotConserved,
}

impl Population {
    /// The weights of the four corners, as a block is written: the top row, then the bottom.
    pub fn weights_text(weights: &[u8; 4]) -> String {
        format!("{}·{}/{}·{}", weights[0], weights[1], weights[2], weights[3])
    }
}

/// The rotations and mirrors of the square under which a rule looks the same: transforming a
/// pattern and then running it gives the transformed run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symmetry {
    /// Every rotation and every mirror.
    Full,
    /// Quarter turns but no mirror: the rule has a handedness.
    Rotations,
    HalfTurnAndAxisMirrors,
    HalfTurnAndDiagonalMirrors,
    HalfTurn,
    LeftRightMirror,
    TopBottomMirror,
    DiagonalMirror,
    None,
}

/// How running a rule backwards relates to running it forwards. Because the partitions
/// alternate, "the same" always means: on the other partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reversed {
    /// The table is its own inverse.
    SameRule,
    /// The inverse is the rule seen turned or in a mirror.
    Transformed,
    /// The inverse is the rule with dead and alive exchanged.
    Complemented,
    /// Both of the above at once.
    TransformedAndComplemented,
    /// No symmetry of the square or of the two states relates the two directions.
    DifferentRule,
}

/// A reversible rule on 2×2 blocks: a permutation of the 16 block states, and its inverse.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlockRule {
    table: [u8; 16],
    inverse: [u8; 16],
}

impl BlockRule {
    pub fn new(table: [u8; 16]) -> Result<Self, RuleError> {
        let mut inverse = [u8::MAX; 16];
        for (input, &output) in table.iter().enumerate() {
            if output > 15 {
                return Err(RuleError::Entry(output.to_string()));
            }
            let earlier = inverse[output as usize];
            if earlier != u8::MAX {
                return Err(RuleError::NotReversible {
                    output,
                    inputs: (earlier, input as u8),
                });
            }
            inverse[output as usize] = input as u8;
        }
        Ok(Self { table, inverse })
    }

    /// The rule that changes nothing.
    pub fn identity() -> Self {
        Self::new(std::array::from_fn(|state| state as u8)).expect("the identity is reversible")
    }

    /// A uniformly random permutation; `random` supplies random bits.
    pub fn random(mut random: impl FnMut() -> u64) -> Self {
        let mut table: [u8; 16] = std::array::from_fn(|state| state as u8);
        for i in (1..table.len()).rev() {
            table.swap(i, (random() % (i as u64 + 1)) as usize);
        }
        Self::new(table).expect("a shuffle is a permutation")
    }

    /// `table()[state]` is the state of the block after one forward step.
    pub fn table(&self) -> &[u8; 16] {
        &self.table
    }

    pub fn table_for(&self, forward: bool) -> &[u8; 16] {
        if forward { &self.table } else { &self.inverse }
    }

    /// The rule that undoes this one.
    pub fn inverted(&self) -> Self {
        Self {
            table: self.inverse,
            inverse: self.table,
        }
    }

    /// Exchanges the outcomes of two block states. This is the elementary edit that keeps a
    /// rule reversible: every permutation can be reached by swaps.
    pub fn swap_outcomes(&mut self, a: u8, b: u8) {
        self.table.swap(a as usize, b as usize);
        for (input, &output) in self.table.iter().enumerate() {
            self.inverse[output as usize] = input as u8;
        }
    }

    /// The preset with this table, if any.
    pub fn preset(&self) -> Option<&'static Preset> {
        PRESETS.iter().find(|preset| preset.table == self.table)
    }

    pub fn name(&self) -> &'static str {
        self.preset().map_or("Custom", |preset| preset.name)
    }

    /// The rule Morita calls ESPCA-`number` (*Reversible World of Cellular Automata*, 2024).
    ///
    /// In an elementary square partitioned automaton every square cell has four parts, each
    /// holding a particle or not, and a cell's next state depends on the parts of its four
    /// neighbours that face it. Put a site on every edge between two cells: a particle about
    /// to cross that edge sits there. A cell then takes in the four sites around it and puts
    /// four out again, which is a block being rewritten; the cells that do so on even steps
    /// and the ones in between on odd steps are the two partitions. So such an automaton is
    /// two block automata that never meet, each drawn turned by 45°: Morita's north is
    /// up and to the right here.
    ///
    /// The six digits give the outcome of a cell with no particle coming in, one, two at a
    /// right angle, two head-on, three and four; quarter turns supply the other cases.
    pub fn from_espca(number: &str) -> Result<Self, RuleError> {
        let not_a_number = || RuleError::Espca(number.to_string());
        let digits: Vec<u8> = number.chars().filter_map(|c| c.to_digit(16)).map(|d| d as u8).collect();
        if digits.len() != 6 || number.chars().count() != 6 {
            return Err(not_a_number());
        }
        let mut table = [u8::MAX; 16];
        for (&incoming, &outgoing) in ESPCA_CASES.iter().zip(&digits) {
            let (mut before, mut after) = (entering(incoming), leaving(outgoing));
            for _ in 0..4 {
                let outcome = &mut table[before as usize];
                // A case that a turn maps onto itself must have such an outcome as well.
                if *outcome != u8::MAX && *outcome != after {
                    return Err(not_a_number());
                }
                *outcome = after;
                (before, after) = (rotate_cw(before), rotate_cw(after));
            }
        }
        Self::new(table)
    }

    /// The rule's number in Morita's notation, if it has one: it must look the same after a
    /// quarter turn.
    pub fn espca(&self) -> Option<String> {
        let digit = |&incoming: &u8| {
            let outgoing = parts_leaving(self.table[entering(incoming) as usize]);
            char::from_digit(outgoing as u32, 16).expect("four bits are a hexadecimal digit")
        };
        self.commutes_with(rotate_cw).then(|| ESPCA_CASES.iter().map(digit).collect())
    }

    /// The empty world through time. All its blocks are alike, so one block state describes
    /// it: the state of every block about to be rewritten. It starts at 0 and is back at 0
    /// after as many generations as the cycle is long, sixteen at most.
    pub fn vacuum_cycle(&self) -> Vec<u8> {
        let mut cycle = vec![0];
        loop {
            // The next step's blocks are shifted by a cell each way, which shows the world's
            // repeating 2×2 tile turned by a half turn.
            let next = rotate_180(self.table[*cycle.last().unwrap() as usize]);
            if next == 0 {
                return cycle;
            }
            cycle.push(next);
        }
    }

    /// The rule as it acts on the difference from the vacuum: one table for each generation
    /// of the vacuum's cycle, every one of them leaving empty blocks empty. For a rule with a
    /// stable vacuum that is the rule itself.
    pub fn relative_to_vacuum(&self) -> Vec<BlockRule> {
        self.vacuum_cycle()
            .into_iter()
            .map(|vacuum| {
                let after = self.table[vacuum as usize];
                let table =
                    std::array::from_fn(|block| self.table[block ^ vacuum as usize] ^ after);
                BlockRule::new(table).expect("a permutation relabelled is a permutation")
            })
            .collect()
    }

    pub fn population(&self) -> Population {
        let conserves = |rule: &BlockRule| {
            (0..16u8).all(|state| popcount(rule.table[state as usize]) == popcount(state))
        };
        if conserves(self) {
            Population::Conserved
        } else if self.relative_to_vacuum().iter().all(conserves) {
            Population::ConservedRelativeToVacuum
        } else if let Some(weights) = self.conserved_weights() {
            Population::Weighted(weights)
        } else {
            Population::NotConserved
        }
    }

    /// The weights under which the rule keeps a weighted number of cells, if there are any. A
    /// cell weighs according to its corner of the block about to be rewritten, and the
    /// weights of all cells add up to the same at every generation ([`keeps_weight`]). Of
    /// all such [`weightings`] these are the lightest: all 1 for a rule that simply keeps
    /// the number of cells.
    ///
    /// Under any other the number of cells changes, a heavy cell for two light ones, but only
    /// within the ratio of the weights: no pattern explodes and none dwindles away.
    pub fn conserved_weights(&self) -> Option<[u8; 4]> {
        let tables = self.relative_to_vacuum();
        weightings().into_iter().find(|weights| tables.iter().all(|rule| keeps_weight(&rule.table, weights)))
    }

    /// Does the rule give the same result whether a block is transformed before or after it?
    pub fn commutes_with(&self, transform: fn(u8) -> u8) -> bool {
        (0..16u8).all(|state| {
            self.table[transform(state) as usize] == transform(self.table[state as usize])
        })
    }

    pub fn symmetry(&self) -> Symmetry {
        let left_right = self.commutes_with(mirror);
        let diagonal = self.commutes_with(transpose) || self.commutes_with(anti_transpose);
        // The transformations a rule commutes with form a group, which leaves these cases.
        if self.commutes_with(rotate_cw) {
            if left_right { Symmetry::Full } else { Symmetry::Rotations }
        } else if self.commutes_with(rotate_180) {
            if left_right {
                Symmetry::HalfTurnAndAxisMirrors
            } else if diagonal {
                Symmetry::HalfTurnAndDiagonalMirrors
            } else {
                Symmetry::HalfTurn
            }
        } else if left_right {
            Symmetry::LeftRightMirror
        } else if self.commutes_with(flip) {
            Symmetry::TopBottomMirror
        } else if diagonal {
            Symmetry::DiagonalMirror
        } else {
            Symmetry::None
        }
    }

    /// Does exchanging dead and alive turn every run into another run?
    pub fn is_complement_symmetric(&self) -> bool {
        self.commutes_with(complement)
    }

    /// The rule as it looks when the plane is turned or mirrored by `transform`.
    pub fn seen_through(&self, transform: fn(u8) -> u8) -> BlockRule {
        let mut table = [0; 16];
        for block in 0..16u8 {
            table[transform(block) as usize] = transform(self.table[block as usize]);
        }
        BlockRule::new(table).expect("a permutation relabelled is a permutation")
    }

    /// The canonical form of the rule: of all the rules that differ from this one only in how
    /// one looks at them, turned or mirrored, begun at another generation of the vacuum's
    /// cycle, or with a vacuum that flickers and changes nothing else, the one whose table
    /// comes first. One table to stand for them all.
    pub fn canonical(&self) -> BlockRule {
        // A rule that is its own complement acts on what differs from its vacuum in one
        // way at every generation: that way is the rule to look at.
        let relative = self.relative_to_vacuum();
        let flickers_only = relative.iter().all(|table| *table == relative[0]);
        let begun = if flickers_only { vec![relative[0].clone()] } else { self.begun_later() };
        let turned = |rule: &BlockRule| TURNS_AND_MIRRORS.map(|transform| rule.seen_through(transform));
        let seen = begun.iter().flat_map(turned).chain(begun.iter().cloned());
        seen.min_by_key(|rule| rule.table).expect("the rule itself is among them")
    }

    /// The rule as it is for a world that begins at a later generation of the vacuum's cycle,
    /// one rule for every generation of it, the first being the rule itself. Under all of
    /// them what differs from the vacuum goes through the same tables, only begun elsewhere:
    /// they are one world, seen some generations apart. Under Critters, whose vacuum flips,
    /// the other one is Critters with dead and alive exchanged.
    pub(crate) fn begun_later(&self) -> Vec<BlockRule> {
        let vacuum = self.vacuum_cycle();
        let later = |(generation, relative): (usize, &BlockRule)| {
            // An empty block has to become the vacuum of the generation after, as the
            // blocks of this one see it.
            let empty = rotate_180(vacuum[generation] ^ vacuum[(generation + 1) % vacuum.len()]);
            BlockRule::new(relative.table.map(|outcome| outcome ^ empty)).expect("a permutation relabelled is a permutation")
        };
        self.relative_to_vacuum().iter().enumerate().map(later).collect()
    }

    pub fn reversed(&self) -> Reversed {
        // Is the inverse the rule as seen through `transform`, with or without the two states
        // exchanged as well?
        let inverse_through = |transform: fn(u8) -> u8, complemented: bool| {
            let through = |block| {
                if complemented { complement(transform(block)) } else { transform(block) }
            };
            (0..16u8).all(|state| {
                self.inverse[through(state) as usize] == through(self.table[state as usize])
            })
        };
        if self.table == self.inverse {
            Reversed::SameRule
        } else if TURNS_AND_MIRRORS.iter().any(|&turn| inverse_through(turn, false)) {
            Reversed::Transformed
        } else if inverse_through(|block| block, true) {
            Reversed::Complemented
        } else if TURNS_AND_MIRRORS.iter().any(|&turn| inverse_through(turn, true)) {
            Reversed::TransformedAndComplemented
        } else {
            Reversed::DifferentRule
        }
    }
}

impl fmt::Display for BlockRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries: Vec<String> = self.table.iter().map(u8::to_string).collect();
        f.write_str(&entries.join(","))
    }
}

impl FromStr for BlockRule {
    type Err = String;

    /// Accepts a preset (`critters`, `Single Rotation`, `hpp-gas`, ...), Morita's number of a
    /// rule (`ESPCA-01c5ef`), or a table of sixteen states separated by commas, semicolons or
    /// spaces, optionally with MCell's `MS,D` prefix.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Presets match however their words are joined: "Single Rotation", "single-rotation".
        let key = |text: &str| -> String {
            text.chars()
                .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '_'))
                .map(|c| c.to_ascii_lowercase())
                .collect()
        };
        let wanted = key(s);
        if let Some(preset) = PRESETS
            .iter()
            .find(|preset| key(preset.id) == wanted || key(preset.name) == wanted)
        {
            return Ok(preset.rule());
        }
        if let Some(number) = wanted.strip_prefix("espca") {
            return Self::from_espca(number).map_err(|error| error.to_string());
        }

        let body = s.trim();
        let body = match body.get(..4) {
            Some(prefix) if prefix.eq_ignore_ascii_case("MS,D") => &body[4..],
            _ => body,
        };
        let entries: Vec<&str> = body
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .filter(|entry| !entry.is_empty())
            .collect();
        let is_number = |entry: &&str| entry.bytes().all(|b| b.is_ascii_digit());
        if entries.is_empty() || !entries.iter().all(is_number) {
            let ids: Vec<_> = PRESETS.iter().map(|preset| preset.id).collect();
            return Err(format!(
                "unknown rule {s:?}; expected 16 block states like \
                 0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15 or one of: {}",
                ids.join(", ")
            ));
        }
        if entries.len() != 16 {
            return Err(RuleError::Length(entries.len()).to_string());
        }
        let mut table = [0u8; 16];
        for (slot, entry) in table.iter_mut().zip(&entries) {
            *slot = entry
                .parse()
                .map_err(|_| RuleError::Entry(entry.to_string()).to_string())?;
        }
        Self::new(table).map_err(|error| error.to_string())
    }
}

/// The ways to turn and mirror a block, other than leaving it alone.
pub const TURNS_AND_MIRRORS: [fn(u8) -> u8; 7] =
    [rotate_cw, rotate_180, rotate_ccw, mirror, flip, transpose, anti_transpose];

/// The cases the digits of an ESPCA number are for, as Morita writes a cell: its parts top,
/// right, bottom and left from the highest bit down. A particle in the top part is moving
/// north.
const ESPCA_CASES: [u8; 6] = [0b0000, 0b0010, 0b0011, 0b1010, 0b0111, 0b1111];

/// The block that a cell of an ESPCA takes in: its cells top-left, top-right, bottom-left and
/// bottom-right hold the particles coming from the west, the north, the south and the east,
/// which are the ones moving east, south, north and west.
const fn entering(parts: u8) -> u8 {
    let (north, east, south, west) = (parts >> 3 & 1, parts >> 2 & 1, parts >> 1 & 1, parts & 1);
    east | south << 1 | north << 2 | west << 3
}

/// The block that a cell of an ESPCA puts out: the same four places, now holding the
/// particles that leave towards the west, the north, the south and the east.
const fn leaving(parts: u8) -> u8 {
    let (north, east, south, west) = (parts >> 3 & 1, parts >> 2 & 1, parts >> 1 & 1, parts & 1);
    west | north << 1 | south << 2 | east << 3
}

/// [`leaving`] the other way round.
const fn parts_leaving(block: u8) -> u8 {
    let (west, north, south, east) = (block & 1, block >> 1 & 1, block >> 2 & 1, block >> 3 & 1);
    north << 3 | east << 2 | south << 1 | west
}

pub const fn popcount(block: u8) -> u32 {
    (block & 0xF).count_ones()
}

/// No cell needs to weigh more than this in a weighted number of cells: heavier weights, tried
/// up to 9, bring no rule that these do not.
pub const HEAVIEST: u8 = 4;

/// Every way to give the corners of a block (top-left, top-right, bottom-left, bottom-right)
/// weights from 1 to [`HEAVIEST`], the lightest first, without those that only repeat a
/// lighter one in larger numbers.
pub fn weightings() -> Vec<[u8; 4]> {
    let weights = || 1..=HEAVIEST;
    let all = weights().flat_map(|a| weights().flat_map(move |b| weights().flat_map(move |c| weights().map(move |d| [a, b, c, d]))));
    let common = |weights: &[u8; 4]| (2..=HEAVIEST).any(|divisor| weights.iter().all(|weight| weight % divisor == 0));
    let mut all: Vec<[u8; 4]> = all.filter(|weights| !common(weights)).collect();
    all.sort_by_key(|weights| weights.iter().map(|&weight| weight as u32).sum::<u32>());
    all
}

/// What the cells of a block weigh together.
pub fn weigh(block: u8, weights: &[u8; 4]) -> u32 {
    (0..4).filter(|corner| block >> corner & 1 == 1).map(|corner| weights[corner] as u32).sum()
}

/// Does every block weigh the same before and after the table rewrites it? Afterwards its
/// cells are weighed where the next step finds them: that step's blocks are shifted by a cell
/// each way, so each cell is in the opposite corner of its new block.
pub fn keeps_weight(table: &[u8; 16], weights: &[u8; 4]) -> bool {
    (0..16u8).all(|block| weigh(rotate_180(table[block as usize]), weights) == weigh(block, weights))
}

/// Exchange dead and alive.
pub const fn complement(block: u8) -> u8 {
    !block & 0xF
}

/// Rotate the block a quarter turn clockwise (with `y` pointing down): TL → TR → BR → BL → TL.
pub const fn rotate_cw(block: u8) -> u8 {
    let tl = block & 1;
    let tr = (block >> 1) & 1;
    let bl = (block >> 2) & 1;
    let br = (block >> 3) & 1;
    bl | (tl << 1) | (br << 2) | (tr << 3)
}

/// Rotate the block by a half turn: TL ↔ BR, TR ↔ BL.
pub const fn rotate_180(block: u8) -> u8 {
    rotate_cw(rotate_cw(block))
}

pub const fn rotate_ccw(block: u8) -> u8 {
    rotate_cw(rotate_180(block))
}

/// Mirror the block left to right: TL ↔ TR, BL ↔ BR.
pub const fn mirror(block: u8) -> u8 {
    ((block & 0b0101) << 1) | ((block & 0b1010) >> 1)
}

/// Mirror the block top to bottom: TL ↔ BL, TR ↔ BR.
pub const fn flip(block: u8) -> u8 {
    ((block & 0b0011) << 2) | ((block & 0b1100) >> 2)
}

/// Mirror the block in its main diagonal: TR ↔ BL.
pub const fn transpose(block: u8) -> u8 {
    mirror(rotate_cw(block))
}

/// Mirror the block in its other diagonal: TL ↔ BR.
pub const fn anti_transpose(block: u8) -> u8 {
    rotate_cw(mirror(block))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(id: &str) -> BlockRule {
        PRESETS.iter().find(|preset| preset.id == id).unwrap().rule()
    }

    fn from_fn(f: impl Fn(u8) -> u8) -> BlockRule {
        BlockRule::new(std::array::from_fn(|state| f(state as u8))).unwrap()
    }

    #[test]
    fn presets_are_reversible_and_distinct() {
        for (i, a) in PRESETS.iter().enumerate() {
            let rule = a.rule();
            assert_eq!(rule.preset().map(|p| p.id), Some(a.id));
            assert_eq!(rule.name(), a.name);
            for b in &PRESETS[i + 1..] {
                assert_ne!(a.id, b.id);
                assert_ne!(a.table, b.table, "{} and {} share a table", a.id, b.id);
            }
        }
    }

    #[test]
    fn single_rotation_is_its_definition() {
        let rule = from_fn(|b| if popcount(b) == 1 { rotate_cw(b) } else { b });
        assert_eq!(rule, preset("single-rotation"));
        assert_eq!(rule.inverted(), from_fn(|b| if popcount(b) == 1 { rotate_ccw(b) } else { b }));
        assert_eq!(rule.population(), Population::Conserved);
        assert_eq!(rule.symmetry(), Symmetry::Rotations, "it has a sense of rotation");
        assert!(!rule.is_complement_symmetric());
        assert_eq!(rule.reversed(), Reversed::Transformed, "a mirror reverses the rotation");
        assert_eq!(rule.vacuum_cycle(), [0]);
        assert_eq!(rule.relative_to_vacuum(), std::slice::from_ref(&rule));
    }

    #[test]
    fn critters_is_its_definition() {
        let rule = from_fn(|b| match popcount(b) {
            2 => b,
            3 => rotate_180(complement(b)),
            _ => complement(b),
        });
        assert_eq!(rule, preset("critters"));
        assert_eq!(rule.population(), Population::ConservedRelativeToVacuum);
        assert_eq!(rule.symmetry(), Symmetry::Full);
        assert!(!rule.is_complement_symmetric());
        assert_eq!(rule.reversed(), Reversed::Complemented);
        assert_eq!(rule.vacuum_cycle(), [0, 15]);
    }

    #[test]
    fn the_other_presets_do_what_their_blurbs_say() {
        let diagonal = |b: u8| b == 6 || b == 9;
        assert_eq!(
            preset("bbm"),
            from_fn(|b| match popcount(b) {
                1 => rotate_180(b),
                2 if diagonal(b) => complement(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("bounce-gas"),
            from_fn(|b| match popcount(b) {
                1 | 3 => rotate_180(b),
                2 if diagonal(b) => complement(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("hpp-gas"),
            from_fn(|b| if diagonal(b) { complement(b) } else { rotate_180(b) })
        );
        assert_eq!(
            preset("tron"),
            from_fn(|b| if b == 0 || b == 15 { complement(b) } else { b })
        );
        assert_eq!(
            preset("rotations"),
            from_fn(|b| match popcount(b) {
                1 | 3 => rotate_cw(b),
                2 => complement(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("double-rotation"),
            from_fn(|b| match popcount(b) {
                1 => rotate_cw(b),
                3 => rotate_ccw(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("string-thing"),
            from_fn(|b| if popcount(b) == 2 { complement(b) } else { b })
        );
        assert_eq!(preset("swap-on-diagonal"), from_fn(rotate_180));

        for id in ["bbm", "bounce-gas", "hpp-gas", "string-thing", "swap-on-diagonal"] {
            let rule = preset(id);
            assert_eq!(rule.population(), Population::Conserved, "{id}");
            assert_eq!(rule.reversed(), Reversed::SameRule, "{id}");
            assert_eq!(rule.symmetry(), Symmetry::Full, "{id}");
        }
        assert_eq!(preset("tron").vacuum_cycle(), [0, 15]);
        assert_eq!(preset("tron").population(), Population::NotConserved);
        assert!(preset("hpp-gas").is_complement_symmetric());
        assert!(!preset("bbm").is_complement_symmetric());
        assert_eq!(preset("rotations").symmetry(), Symmetry::Rotations);
        assert_eq!(preset("double-rotation").reversed(), Reversed::Transformed);
    }

    /// The rule as it looks in a mirror.
    fn mirrored(rule: &BlockRule) -> BlockRule {
        from_fn(|b| mirror(rule.table()[mirror(b) as usize]))
    }

    #[test]
    fn the_rules_of_the_book_do_what_their_blurbs_say() {
        let diagonal = |b: u8| b == 6 || b == 9;
        assert_eq!(
            preset("espca-01c5ef"),
            from_fn(|b| match popcount(b) {
                1 => rotate_ccw(b),
                2 if diagonal(b) => complement(b),
                3 => rotate_cw(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("espca-01caef"),
            from_fn(|b| match popcount(b) {
                1 => rotate_ccw(b),
                3 => rotate_cw(b),
                _ => b,
            })
        );
        assert_eq!(preset("espca-01caef"), mirrored(&preset("double-rotation")));
        assert_eq!(
            preset("espca-02c5bf"),
            from_fn(|b| match popcount(b) {
                1 => rotate_180(b),
                2 if diagonal(b) => complement(b),
                3 => rotate_ccw(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("espca-016a7f"),
            from_fn(|b| match popcount(b) {
                1 => rotate_ccw(b),
                2 if !diagonal(b) => rotate_cw(b),
                3 => rotate_180(b),
                _ => b,
            })
        );
        // A lone cell gains the next cell counter-clockwise as a neighbour. Of two side by
        // side, the one that could have gained the other so, and the one it would have gained.
        let joined = |b: u8| b | rotate_ccw(b);
        let first = |b: u8| b & rotate_cw(b);
        let second = |b: u8| b & rotate_ccw(b);
        assert_eq!(
            preset("espca-0945df"),
            from_fn(|b| match popcount(b) {
                1 => joined(b),
                2 if diagonal(b) => complement(b),
                2 => first(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("espca-09457f"),
            from_fn(|b| match popcount(b) {
                1 => joined(b),
                2 if diagonal(b) => complement(b),
                2 => first(b),
                3 => rotate_180(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("espca-098aef"),
            from_fn(|b| match popcount(b) {
                1 => joined(b),
                2 if !diagonal(b) => second(b),
                3 => rotate_cw(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("espca-0925bf"),
            from_fn(|b| match popcount(b) {
                1 => joined(b),
                2 if diagonal(b) => complement(b),
                2 => rotate_180(second(b)),
                3 => rotate_ccw(b),
                _ => b,
            })
        );
        assert_eq!(
            preset("espca-0dca8f"),
            from_fn(|b| match popcount(b) {
                1 | 3 => complement(rotate_180(b)),
                _ => b,
            })
        );

        for id in ["espca-01c5ef", "espca-01caef", "espca-02c5bf", "espca-016a7f"] {
            assert_eq!(preset(id).population(), Population::Conserved, "{id}");
            assert_eq!(preset(id).symmetry(), Symmetry::Rotations, "{id}");
        }
        for id in ["espca-0945df", "espca-09457f", "espca-098aef", "espca-0925bf", "espca-0dca8f"] {
            assert_eq!(preset(id).population(), Population::NotConserved, "{id}");
            assert_eq!(preset(id).vacuum_cycle(), [0], "{id}");
        }
        // A gun that fires in either direction of time: the rule is its own inverse.
        assert_eq!(preset("espca-09457f").reversed(), Reversed::SameRule);
    }

    /// How the found rules are made, as the README has it; their blurbs say what they do,
    /// which the tests of the search hold them to.
    #[test]
    fn the_found_rules_are_made_as_described() {
        // The diagonal that lone cells gain and lose: top-left and bottom-right.
        let other_diagonal = 9;
        let side_by_side = |b: u8| popcount(b) == 2 && b != 6 && b != 9;
        let steady = from_fn(|b| match b {
            2 | 4 | 11 | 13 => b ^ other_diagonal,
            b if side_by_side(b) => rotate_ccw(b),
            b => b,
        });
        assert_eq!(preset("steady-blob"), steady);
        assert_eq!(
            preset("creeping-blob"),
            from_fn(|b| match b {
                6 | 15 => b ^ other_diagonal,
                7 | 14 => rotate_180(b),
                b => steady.table()[b as usize],
            })
        );
        assert_eq!(
            preset("ship-factory"),
            from_fn(|b| match b {
                2 | 4 | 11 | 13 => b ^ other_diagonal,
                6 | 9 => complement(b),
                b if side_by_side(b) => anti_transpose(b),
                b => b,
            })
        );
        // Single Rotation's lone cells, five pairs in a cycle, two blocks of three swapped.
        let cycle = [3, 5, 6, 12, 10];
        assert_eq!(
            preset("diagonal-traffic"),
            from_fn(|b| match (popcount(b), cycle.iter().position(|&pair| pair == b)) {
                (1, _) => rotate_cw(b),
                (_, Some(place)) => cycle[(place + 1) % cycle.len()],
                _ if b == 7 || b == 14 => rotate_180(b),
                _ => b,
            })
        );

        for id in ["steady-blob", "creeping-blob", "ship-factory", "plus-ships", "four-way-gun"] {
            assert_eq!(preset(id).population(), Population::NotConserved, "{id}");
            assert_eq!(preset(id).conserved_weights(), None, "{id}");
        }
        assert_eq!(preset("plus-ships").vacuum_cycle(), [0, 15, 6, 9]);
        assert_eq!(preset("four-way-gun").vacuum_cycle(), [0, 15]);
        assert_eq!(preset("four-way-gun").espca().as_deref(), Some("f6b580"));
        assert_eq!(preset("crossing-fleets").population(), Population::ConservedRelativeToVacuum);
        assert_eq!(preset("crossing-fleets").vacuum_cycle(), [0, 15]);
        assert_eq!(preset("diagonal-traffic").population(), Population::Conserved);
        // None of them is a rule of the literature, or another of them, seen another way.
        for found in PRESETS.iter().filter(|preset| preset.source == Source::Search) {
            for other in PRESETS.iter().filter(|other| other.id != found.id) {
                let (a, b) = (found.rule().canonical(), other.rule().canonical());
                assert_ne!(a, b, "{} and {}", found.id, other.id);
            }
        }
    }

    #[test]
    fn morita_numbers_name_the_rules_that_look_the_same_after_a_quarter_turn() {
        // Of the 65 536 ESPCAs, 1536 are reversible, and 128 of those conserve their
        // particles (Theorem 2.3 of the book).
        let (mut reversible, mut conservative) = (0, 0);
        for n in 0..1u32 << 16 {
            let (u, z) = ([0, 0xf][(n & 1) as usize], [0, 0xf][(n >> 1 & 1) as usize]);
            let x = [0, 5, 0xa, 0xf][(n >> 2 & 3) as usize];
            let (v, w, y) = (n >> 4 & 0xf, n >> 8 & 0xf, n >> 12);
            let number = format!("{u:x}{v:x}{w:x}{x:x}{y:x}{z:x}");
            match BlockRule::from_espca(&number) {
                Ok(rule) => {
                    reversible += 1;
                    conservative += (rule.population() == Population::Conserved) as u32;
                    assert!(rule.commutes_with(rotate_cw), "{number}");
                    assert_eq!(rule.espca(), Some(number));
                }
                Err(error) => {
                    assert!(matches!(error, RuleError::NotReversible { .. }), "{number}: {error}");
                }
            }
        }
        assert_eq!((reversible, conservative), (1536, 128));

        // A rule without the symmetry has no number.
        let mut lopsided = BlockRule::identity();
        lopsided.swap_outcomes(1, 2);
        assert_eq!(lopsided.espca(), None);
        // Every particle turning back is nothing happening at all.
        assert_eq!(BlockRule::from_espca("08cadf"), Ok(BlockRule::identity()));
    }

    #[test]
    fn the_presets_have_the_numbers_the_book_gives_them() {
        // Morita finds his ESPCA-02c5df to be Margolus' automaton (Sec. 6.3), and
        // ESPCA-04cabf to be the mirror image of ESPCA-01caef (Sec. 5.2.1).
        assert_eq!(preset("bbm").espca().as_deref(), Some("02c5df"));
        assert_eq!(preset("double-rotation").espca().as_deref(), Some("04cabf"));
        // The mirror images of his four universal rules, as listed in Sec. 6.5.
        for (number, in_a_mirror) in
            [("01c5ef", "04c5bf"), ("01caef", "04cabf"), ("02c5df", "02c5df"), ("02c5bf", "02c5ef")]
        {
            let rule = BlockRule::from_espca(number).unwrap();
            assert_eq!(mirrored(&rule).espca().as_deref(), Some(in_a_mirror), "{number}");
        }
        assert_eq!(preset("single-rotation").espca().as_deref(), Some("04cadf"));
        assert_eq!(preset("critters").espca().as_deref(), Some("f7ca80"));

        for preset in PRESETS.iter().filter(|preset| preset.source == Source::Morita) {
            let number = preset.id.strip_prefix("espca-").unwrap();
            assert_eq!(BlockRule::from_espca(number), Ok(preset.rule()), "{}", preset.id);
            assert_eq!(preset.name, format!("ESPCA-{number}"));
        }
    }

    #[test]
    fn one_rule_stands_for_all_that_only_look_different() {
        // A mirror image, a quarter turn of a rule without that symmetry, and the rule itself.
        let rule = BlockRule::from_espca("01caef").unwrap();
        let canonical = rule.canonical();
        assert_eq!(mirrored(&rule).canonical(), canonical);
        assert_eq!(preset("double-rotation").canonical(), canonical);
        assert_eq!(canonical.canonical(), canonical);
        let mut lopsided = BlockRule::identity();
        lopsided.swap_outcomes(1, 3);
        for transform in TURNS_AND_MIRRORS {
            assert_eq!(lopsided.seen_through(transform).canonical(), lopsided.canonical());
        }
        assert_eq!(rule.seen_through(rotate_cw), rule, "it looks the same after a quarter turn");
        // ESPCA-fb3510 is its own complement, and its vacuum only flickers: it is ESPCA-04caef.
        let flickering = BlockRule::from_espca("fb3510").unwrap();
        let plain = BlockRule::from_espca("04caef").unwrap();
        assert_eq!(flickering.canonical(), plain.canonical());
        assert_eq!(flickering.canonical().vacuum_cycle(), [0]);
        // Critters is not: there the two generations differ. It is one world with the rule
        // that has dead and alive exchanged, which is Critters begun a generation later,
        // and that one's table comes first.
        let critters = preset("critters");
        let exchanged = from_fn(|b| complement(critters.table()[complement(b) as usize]));
        assert_eq!(critters.begun_later(), [critters.clone(), exchanged.clone()]);
        assert_eq!(critters.canonical(), exchanged);
        assert_eq!(exchanged.canonical(), exchanged);
        // So are two guns of the search: ESPCA-f6b580 and ESPCA-fd1560.
        let gun = BlockRule::from_espca("f6b580").unwrap();
        assert_eq!(BlockRule::from_espca("fd1560").unwrap().canonical(), gun.canonical());
    }

    #[test]
    fn a_rule_begun_later_takes_the_same_tables_from_there() {
        let mut state = 99u64;
        let mut random = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            state >> 33
        };
        let presets = PRESETS.iter().map(|preset| preset.rule());
        let rules: Vec<BlockRule> = presets.chain((0..200).map(|_| BlockRule::random(&mut random))).collect();
        for rule in &rules {
            let relative = rule.relative_to_vacuum();
            let begun = rule.begun_later();
            assert_eq!(begun[0], *rule);
            for (generation, later) in begun.iter().enumerate() {
                let mut expected = relative.clone();
                expected.rotate_left(generation);
                assert_eq!(later.relative_to_vacuum(), expected, "{rule} from generation {generation}");
                assert_eq!(later.canonical(), rule.canonical(), "{rule} from generation {generation}");
            }
        }
    }

    #[test]
    fn every_kind_of_symmetry_is_told_apart() {
        // The identity with two outcomes exchanged keeps exactly the symmetries that map the
        // pair of blocks onto itself. Blocks: 1 top-left, 2 top-right, 4 bottom-left,
        // 8 bottom-right, 3 top row, 12 bottom row.
        let swapped = |a, b| {
            let mut rule = BlockRule::identity();
            rule.swap_outcomes(a, b);
            rule
        };
        assert_eq!(BlockRule::identity().symmetry(), Symmetry::Full);
        assert_eq!(swapped(1, 2).symmetry(), Symmetry::LeftRightMirror);
        assert_eq!(swapped(1, 4).symmetry(), Symmetry::TopBottomMirror);
        assert_eq!(swapped(1, 3).symmetry(), Symmetry::None);
        assert_eq!(swapped(2, 4).symmetry(), Symmetry::HalfTurnAndDiagonalMirrors);
        assert_eq!(swapped(3, 12).symmetry(), Symmetry::HalfTurnAndAxisMirrors);
        assert_eq!(swapped(1, 7).symmetry(), Symmetry::DiagonalMirror);

        // Top-left and bottom-right trade places, and so do the two rows: only the half turn
        // maps both exchanges onto themselves.
        let mut half_turn = swapped(1, 8);
        half_turn.swap_outcomes(3, 12);
        assert_eq!(half_turn.symmetry(), Symmetry::HalfTurn);
    }

    #[test]
    fn reversal_is_classified() {
        assert_eq!(BlockRule::identity().reversed(), Reversed::SameRule);
        // A three-cycle of single cells is not its own inverse; a mirror that exchanges two of
        // its blocks runs it the other way.
        let mut cycle = BlockRule::identity();
        cycle.swap_outcomes(1, 2);
        cycle.swap_outcomes(2, 4);
        assert_eq!(cycle.reversed(), Reversed::Transformed);
        for seed in 1..40u64 {
            let mut state = seed;
            let rule = BlockRule::random(|| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                state >> 33
            });
            // Whatever relates a rule to its inverse relates the inverse to the rule.
            assert_eq!(rule.reversed(), rule.inverted().reversed(), "{rule}");
        }
    }

    #[test]
    fn every_rule_has_a_vacuum_it_leaves_alone() {
        let mut state = 11u64;
        let mut next = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            state >> 33
        };
        let random = (0..200).map(|_| BlockRule::random(&mut next));
        let mut longest = 0;
        for rule in PRESETS.iter().map(|preset| preset.rule()).chain(random) {
            let cycle = rule.vacuum_cycle();
            let relative = rule.relative_to_vacuum();
            assert!((1..=16).contains(&cycle.len()), "{rule}: {cycle:?}");
            assert_eq!(cycle[0], 0);
            assert_eq!(relative.len(), cycle.len());
            for (i, table) in relative.iter().enumerate() {
                assert_eq!(table.table()[0], 0, "{rule}: generation {i} disturbs the vacuum");
                // The difference `d` from the vacuum `v` evolves as the cells `d ^ v` do.
                let (v, after) = (cycle[i], rule.table()[cycle[i] as usize]);
                for d in 0..16u8 {
                    assert_eq!(table.table()[d as usize] ^ after, rule.table()[(d ^ v) as usize]);
                }
            }
            longest = longest.max(cycle.len());
        }
        assert!(longest > 2, "random rules have vacua beyond stable and flipping");
    }

    #[test]
    fn inverses_undo_the_rules() {
        for preset in &PRESETS {
            let rule = preset.rule();
            let inverse = rule.inverted();
            for state in 0..16usize {
                assert_eq!(inverse.table()[rule.table()[state] as usize], state as u8);
                assert_eq!(rule.table_for(false)[rule.table_for(true)[state] as usize], state as u8);
            }
            assert_eq!(inverse.inverted(), rule);
        }
    }

    #[test]
    fn swapping_outcomes_keeps_a_rule_reversible() {
        // Single rotation is three swaps away from the identity.
        let mut rule = BlockRule::identity();
        rule.swap_outcomes(1, 2);
        rule.swap_outcomes(2, 8);
        rule.swap_outcomes(8, 4);
        assert_eq!(rule, preset("single-rotation"));
        assert_eq!(BlockRule::new(*rule.table()), Ok(rule.clone()));
        assert_eq!(rule.inverted().inverted(), rule);
    }

    #[test]
    fn random_rules_are_permutations() {
        let mut state = 7u64;
        let mut next = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            state >> 33
        };
        let rules: Vec<_> = (0..20).map(|_| BlockRule::random(&mut next)).collect();
        for rule in &rules {
            assert_eq!(BlockRule::new(*rule.table()).as_ref(), Ok(rule));
        }
        assert!(rules.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn rules_round_trip_through_text() {
        for preset in &PRESETS {
            let rule = preset.rule();
            assert_eq!(rule.to_string().parse(), Ok(rule.clone()));
            assert_eq!(preset.id.parse(), Ok(rule.clone()));
            assert_eq!(preset.name.parse(), Ok(rule));
        }
        let single_rotation = preset("single-rotation");
        assert_eq!(single_rotation.to_string(), "0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15");
        assert_eq!("SingleRotation".parse(), Ok(single_rotation.clone()));
        assert_eq!(
            "MS,D0;2;8;3;1;5;6;7;4;9;10;11;12;13;14;15".parse(),
            Ok(single_rotation.clone())
        );
        assert_eq!(
            "Ms,d0;2;8;3;1;5;6;7;4;9;10;11;12;13;14;15".parse(),
            Ok(single_rotation.clone())
        );
        assert_eq!(" 0 2 8 3 1 5 6 7  4 9 10 11 12 13 14 15 ".parse(), Ok(single_rotation.clone()));
        // Morita's numbers, also of rules that are not presets.
        for text in ["ESPCA-04cadf", "espca-04cadf", "espca 04CADF", "Espca04cadf"] {
            assert_eq!(text.parse(), Ok(single_rotation.clone()), "{text}");
        }
    }

    #[test]
    fn bad_rules_are_explained() {
        let parse = |text: &str| text.parse::<BlockRule>().unwrap_err();
        assert!(parse("life").contains("unknown rule"));
        assert!(parse("").contains("unknown rule"));
        assert!(parse("0,1,2").contains("16 entries, found 3"));
        assert!(parse("0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,16").contains("not a block state"));
        assert!(parse("0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,99999999999").contains("not a block state"));
        assert!(parse("+0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15").contains("unknown rule"));
        assert!(parse("t,r,o,n").contains("unknown rule"));
        // The "sand" rule of the MCell collection is not reversible.
        assert_eq!(
            parse("0,4,8,12,4,12,12,13,8,12,12,14,12,13,14,15"),
            "not reversible: blocks 1 and 4 both become 4"
        );
        assert_eq!(
            BlockRule::new([0; 16]),
            Err(RuleError::NotReversible { output: 0, inputs: (0, 1) })
        );
        // The book's example of an irreversible ESPCA (Example 2.2), and numbers that are
        // none: too short, or without the symmetry.
        assert!(parse("espca-09458f").starts_with("not reversible"));
        for text in ["espca-01c5e", "espca-01c5eff", "espca-11c5ef", "espca-01c1ef", "espca-01c5eg"] {
            assert!(parse(text).contains("is not the number of an ESPCA"), "{text}");
        }
    }

    #[test]
    fn block_transforms_compose() {
        for state in 0..16u8 {
            assert_eq!(rotate_cw(rotate_cw(rotate_cw(rotate_cw(state)))), state);
            assert_eq!(mirror(mirror(state)), state);
            assert_eq!(popcount(rotate_cw(state)), popcount(state));
            assert_eq!(popcount(mirror(state)), popcount(state));
            assert_eq!(popcount(complement(state)), 4 - popcount(state));
        }
        assert_eq!(rotate_cw(0b0001), 0b0010);
        assert_eq!(rotate_180(0b0001), 0b1000);
        assert_eq!(mirror(0b0001), 0b0010);
        assert_eq!(mirror(0b0100), 0b1000);
        assert_eq!(flip(0b0001), 0b0100);
        assert_eq!(flip(0b0010), 0b1000);
        assert_eq!((transpose(0b0010), transpose(0b0001)), (0b0100, 0b0001));
        assert_eq!((anti_transpose(0b0001), anti_transpose(0b0010)), (0b1000, 0b0010));
        assert_eq!(rotate_ccw(rotate_cw(0b0110)), 0b0110);
    }
}
