//! Families of rules: the places a search can look.
//!
//! There are 16! reversible rules, far too many to go through, and nearly all of them turn
//! everything into noise. A family is a set small enough to go through and chosen by a
//! property that holds the noise off: a symmetry, or something the rule conserves.

use std::collections::HashSet;

use crate::{
    rules::{BlockRule, Population, complement, keeps_weight, popcount, rotate_180, weigh, weightings},
    universe::Rng,
};

/// Every rule that looks the same when the plane is turned or mirrored by `transform`: with
/// a quarter turn Morita's 1536 ESPCAs, with a half turn or a mirror 1 105 920 rules.
pub fn symmetric_under(transform: fn(u8) -> u8) -> Vec<BlockRule> {
    // The transform takes the blocks around in cycles. A rule that commutes with it takes
    // every cycle to a cycle of the same length, beginning anywhere in it.
    let mut cycles: Vec<Vec<u8>> = Vec::new();
    for block in 0..16u8 {
        if cycles.iter().any(|cycle| cycle.contains(&block)) {
            continue;
        }
        let mut cycle = vec![block];
        while transform(*cycle.last().unwrap()) != block {
            cycle.push(transform(*cycle.last().unwrap()));
        }
        cycles.push(cycle);
    }
    let mut tables = vec![[0u8; 16]];
    for length in 1..=16 {
        let alike: Vec<&Vec<u8>> = cycles.iter().filter(|cycle| cycle.len() == length).collect();
        let places: Vec<u8> = (0..alike.len() as u8).collect();
        let beginnings = length.pow(alike.len() as u32);
        let mut longer = Vec::with_capacity(tables.len());
        for table in &tables {
            for order in permutations(&places) {
                for mut beginning in 0..beginnings {
                    let mut table = *table;
                    for (cycle, &place) in alike.iter().zip(&order) {
                        for (step, &block) in cycle.iter().enumerate() {
                            table[block as usize] = alike[place as usize][(step + beginning) % length];
                        }
                        beginning /= length;
                    }
                    longer.push(table);
                }
            }
        }
        tables = longer;
    }
    tables.into_iter().map(|table| BlockRule::new(table).expect("a permutation")).collect()
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

/// Every rule that keeps a weighted number of cells ([`BlockRule::conserved_weights`]) and
/// not their plain number. Cells are made and unmade under these, and still nothing can
/// explode or dwindle away.
pub fn weighted() -> Vec<BlockRule> {
    let mut rules = Vec::new();
    for weights in weightings() {
        // A block may turn into any block that weighs as much, seen from the step after.
        let after = |weight: u32| -> Vec<u8> { (0..16).filter(|&block| weigh(rotate_180(block), &weights) == weight).collect() };
        let mut tables = vec![[0u8; 16]];
        for weight in 0..=weigh(15, &weights) {
            let blocks: Vec<u8> = (0..16).filter(|&block| weigh(block, &weights) == weight).collect();
            let outcomes = after(weight);
            if outcomes.len() != blocks.len() {
                tables.clear();
            }
            let mut longer = Vec::new();
            for table in &tables {
                for order in permutations(&outcomes) {
                    let mut table = *table;
                    for (&block, &outcome) in blocks.iter().zip(&order) {
                        table[block as usize] = outcome;
                    }
                    longer.push(table);
                }
            }
            tables = longer;
        }
        debug_assert!(tables.iter().all(|table| keeps_weight(table, &weights)));
        let found = tables.into_iter().map(|table| BlockRule::new(table).expect("a permutation"));
        rules.extend(found.filter(|rule| matches!(rule.population(), Population::Weighted(_))));
    }
    rules
}

/// So many rules, each a random permutation of the sixteen blocks.
pub fn random(count: usize, seed: u64) -> Vec<BlockRule> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| BlockRule::random(|| rng.next_u64())).collect()
}

/// One rule for each set of rules that differ only in how one looks at them
/// ([`BlockRule::canonical`]), in the order they first come up.
pub fn distinct(rules: impl IntoIterator<Item = BlockRule>) -> Vec<BlockRule> {
    let mut seen = HashSet::new();
    let canonical = rules.into_iter().map(|rule| rule.canonical());
    canonical.filter(|rule| seen.insert(rule.clone())).collect()
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
    use crate::rules::{mirror, rotate_cw};

    #[test]
    fn the_symmetric_families_are_the_rules_with_that_symmetry() {
        let turning = symmetric_under(rotate_cw);
        assert_eq!(turning.len(), 1536);
        assert!(turning.iter().all(|rule| rule.espca().is_some()));
        let unique: HashSet<&BlockRule> = turning.iter().collect();
        assert_eq!(unique.len(), turning.len());
        // Mirror images are one rule, and so are a rule whose vacuum only flickers and a
        // rule begun a generation later.
        let distinct_turning = distinct(turning);
        assert_eq!(distinct_turning.len(), 584);
        assert!(distinct_turning.iter().all(|rule| rule.canonical() == *rule));

        for transform in [rotate_180, mirror] {
            let family = symmetric_under(transform);
            assert_eq!(family.len(), 1_105_920);
            assert!(family.iter().step_by(997).all(|rule| rule.commutes_with(transform)));
            let unique: HashSet<&BlockRule> = family.iter().collect();
            assert_eq!(unique.len(), family.len());
        }
    }

    #[test]
    fn the_conserving_family_keeps_the_cells_of_a_pattern() {
        let conserving = conserving();
        assert_eq!(conserving.len(), 829_440);
        let unique: HashSet<&BlockRule> = conserving.iter().collect();
        assert_eq!(unique.len(), conserving.len());
        for rule in conserving.iter().step_by(997) {
            assert_ne!(rule.population(), Population::NotConserved, "{rule}");
            assert_eq!(rule.conserved_weights(), Some([1; 4]), "{rule}");
        }
        assert_eq!(random(5, 1), random(5, 1));
        assert_eq!(permutations(&[1, 2, 3]).len(), 6);
    }

    #[test]
    fn the_weighted_family_keeps_a_weight_and_not_the_cells() {
        let tables: HashSet<BlockRule> = weighted().into_iter().collect();
        assert_eq!(tables.len(), 107_664);
        let weighted = distinct(tables);
        assert_eq!(weighted.len(), 13_746);
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
        assert!(weighted.contains(&rule.canonical()));
        // Most rules keep no weight at all.
        assert_eq!("espca-0925bf".parse::<BlockRule>().unwrap().conserved_weights(), None);
        assert_eq!("critters".parse::<BlockRule>().unwrap().conserved_weights(), Some([1; 4]));
    }
}
