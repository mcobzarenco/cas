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
//! Every rule here is a bijection on the 16 states, so it has an exact inverse: running time
//! backwards means applying the inverse table with the partition the forward step used.

use std::{fmt, str::FromStr};

/// The rules available in the sandbox.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RuleKind {
    /// Blocks with exactly one live cell rotate 90° clockwise; every other block is left alone.
    /// <https://dmishin.blogspot.com/2013/11/the-single-rotation-rule-remarkably.html>
    #[default]
    SingleRotation,
    /// Margolus' "Critters": blocks with 0, 1 or 4 live cells are complemented, blocks with 2 are
    /// kept, blocks with 3 are complemented and rotated by 180°.
    /// <https://en.wikipedia.org/wiki/Critters_(cellular_automaton)>
    Critters,
}

impl RuleKind {
    pub const ALL: [RuleKind; 2] = [RuleKind::SingleRotation, RuleKind::Critters];

    pub fn name(self) -> &'static str {
        match self {
            Self::SingleRotation => "Single rotation",
            Self::Critters => "Critters",
        }
    }

    /// Stable identifier, used to name the UI widgets so the test rig can click them.
    pub fn id(self) -> &'static str {
        match self {
            Self::SingleRotation => "RuleSingleRotation",
            Self::Critters => "RuleCritters",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::SingleRotation => {
                "Blocks with exactly one live cell rotate 90° clockwise. \
                 Population is conserved and the vacuum is stable."
            }
            Self::Critters => {
                "0, 1 or 4 live cells: invert the block. 2: keep it. 3: invert and rotate 180°. \
                 Every empty block fills up, so the vacuum flips on every step."
            }
        }
    }

    pub fn rule(self) -> BlockRule {
        match self {
            Self::SingleRotation => BlockRule::from_fn(self, |block| {
                if popcount(block) == 1 {
                    rotate_cw(block)
                } else {
                    block
                }
            }),
            Self::Critters => BlockRule::from_fn(self, |block| match popcount(block) {
                2 => block,
                3 => rotate_180(complement(block)),
                _ => complement(block),
            }),
        }
    }
}

impl fmt::Display for RuleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for RuleKind {
    type Err = String;

    /// Lenient: `single-rotation`, `SingleRotation`, `rotation`, `critters`, ...
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key: String = s
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match key.as_str() {
            "singlerotation" | "rotation" | "single" => Ok(Self::SingleRotation),
            "critters" | "critter" => Ok(Self::Critters),
            _ => {
                let names: Vec<_> = Self::ALL.iter().map(|kind| kind.name()).collect();
                Err(format!(
                    "unknown rule {s:?}; expected one of: {}",
                    names.join(", ")
                ))
            }
        }
    }
}

/// A reversible rule on 2×2 blocks: a permutation of the 16 block states and its inverse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRule {
    pub kind: RuleKind,
    /// `table[state]` is the state of the block after one forward step.
    pub table: [u8; 16],
    /// `inverse[table[state]] == state`.
    pub inverse: [u8; 16],
    /// `true` when the empty block maps to the full block, i.e. the background alternates
    /// between all-dead and all-alive on consecutive generations.
    pub vacuum_flips: bool,
}

impl BlockRule {
    /// Panics if `table` is not a permutation, since such a rule would not be reversible.
    pub fn from_table(kind: RuleKind, table: [u8; 16]) -> Self {
        let mut inverse = [u8::MAX; 16];
        for (input, &output) in table.iter().enumerate() {
            assert!(output < 16, "{kind}: block state {output} out of range");
            assert!(
                inverse[output as usize] == u8::MAX,
                "{kind} is not reversible: states {} and {input} both map to {output}",
                inverse[output as usize]
            );
            inverse[output as usize] = input as u8;
        }
        Self {
            kind,
            table,
            inverse,
            vacuum_flips: table[0] == 0b1111,
        }
    }

    pub fn from_fn(kind: RuleKind, f: impl Fn(u8) -> u8) -> Self {
        let mut table = [0u8; 16];
        for (state, out) in table.iter_mut().enumerate() {
            *out = f(state as u8);
        }
        Self::from_table(kind, table)
    }

    pub fn table_for(&self, forward: bool) -> &[u8; 16] {
        if forward { &self.table } else { &self.inverse }
    }
}

pub const fn popcount(block: u8) -> u32 {
    (block & 0xF).count_ones()
}

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
    let tl = block & 1;
    let tr = (block >> 1) & 1;
    let bl = (block >> 2) & 1;
    let br = (block >> 3) & 1;
    br | (bl << 1) | (tr << 2) | (tl << 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numeric code published on dmishin's blog, same bit layout as ours.
    const SINGLE_ROTATION: [u8; 16] = [0, 2, 8, 3, 1, 5, 6, 7, 4, 9, 10, 11, 12, 13, 14, 15];
    /// Critters as tabulated by Toffoli & Margolus (and MCell).
    const CRITTERS: [u8; 16] = [15, 14, 13, 3, 11, 5, 6, 1, 7, 9, 10, 2, 12, 4, 8, 0];

    #[test]
    fn single_rotation_matches_published_table() {
        assert_eq!(RuleKind::SingleRotation.rule().table, SINGLE_ROTATION);
    }

    #[test]
    fn critters_matches_published_table() {
        assert_eq!(RuleKind::Critters.rule().table, CRITTERS);
    }

    #[test]
    fn inverses_undo_the_rules() {
        for kind in RuleKind::ALL {
            let rule = kind.rule();
            for state in 0..16u8 {
                assert_eq!(rule.inverse[rule.table[state as usize] as usize], state, "{kind}");
                assert_eq!(rule.table[rule.inverse[state as usize] as usize], state, "{kind}");
            }
        }
    }

    #[test]
    fn single_rotation_inverse_is_counterclockwise() {
        let rule = RuleKind::SingleRotation.rule();
        for state in 0..16u8 {
            let expected = if popcount(state) == 1 {
                rotate_cw(rotate_cw(rotate_cw(state)))
            } else {
                state
            };
            assert_eq!(rule.inverse[state as usize], expected);
        }
    }

    #[test]
    fn critters_is_not_an_involution_but_flips_the_vacuum() {
        let rule = RuleKind::Critters.rule();
        assert_ne!(rule.table, rule.inverse);
        assert!(rule.vacuum_flips);
        assert!(!RuleKind::SingleRotation.rule().vacuum_flips);
    }

    #[test]
    fn rotations_compose() {
        for state in 0..16u8 {
            assert_eq!(rotate_cw(rotate_cw(state)), rotate_180(state));
            assert_eq!(rotate_180(rotate_180(state)), state);
            assert_eq!(popcount(rotate_cw(state)), popcount(state));
        }
    }

    #[test]
    #[should_panic(expected = "not reversible")]
    fn non_bijective_tables_are_rejected() {
        BlockRule::from_table(RuleKind::Critters, [0; 16]);
    }
}
