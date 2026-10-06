//! The canonical rules among all 16! tables, counted ([`super::EVERY_RULE`]).
//!
//! A rule f is a table R that leaves the empty block alone, its outcomes all changed by
//! e = f(0): f(b) = R(b) ⊕ e. Its world is R relative to each state of the vacuum's cycle, cut
//! where it repeats: L tables. The rules of one world are those whose world is this one turned
//! or mirrored and begun at another of its tables, so a world has 8·L·N/S rules, where N is how
//! many e give R this very world and S how many turns and shifts leave it as it is. One over
//! that, summed over every rule, counts the worlds.
//!
//! Unless R is special, N and S are 1 and L is as long as the vacuum's cycle, which is as
//! likely to be any length from 1 to 16: that makes 15!/8 · (1 + 1/2 + … + 1/16) worlds. R is
//! special if it has an affine direction w, R(b ⊕ w) = R(b) ⊕ R(w), or if a turn or mirror T
//! takes it to itself relative to some block v, T R T⁻¹ = R relative to v. The special R are
//! few enough to be gone through one by one, every e with each.

use std::collections::HashSet;

use rayon::prelude::*;

use crate::{
    families::{EVERY_RULE, Family, Progress},
    rules::{BlockRule, anti_transpose, flip, mirror, rotate_180, rotate_ccw, rotate_cw, seen_through, transpose},
    universe::Rng,
};

type Table = [u8; 16];

const D4: [fn(u8) -> u8; 8] = [|b| b, rotate_cw, rotate_180, rotate_ccw, mirror, flip, transpose, anti_transpose];

/// The table relative to the block v: what it does to what differs from v.
fn relative(table: &Table, v: u8) -> Table {
    std::array::from_fn(|b| table[b ^ v as usize] ^ table[v as usize])
}

/// How long the vacuum's cycle of the rule (R, e) is, and its world.
fn world(r: &Table, e: u8) -> (usize, Vec<Table>) {
    let mut cycle = vec![0u8];
    loop {
        let next = rotate_180(r[*cycle.last().unwrap() as usize] ^ e);
        if next == 0 {
            break;
        }
        cycle.push(next);
    }
    let tables: Vec<Table> = cycle.iter().map(|&v| relative(r, v)).collect();
    let long = tables.len();
    let period =
        (1..=long).find(|&p| long.is_multiple_of(p) && (0..long).all(|i| tables[i] == tables[(i + p) % long])).unwrap();
    (long, tables[..period].to_vec())
}

/// How many turns and shifts leave a world as it is.
fn stabiliser(world: &[Table]) -> u64 {
    let long = world.len();
    let leaves = |turn: fn(u8) -> u8, shift: usize| {
        (0..long).all(|i| seen_through(&world[i], turn) == world[(i + shift) % long])
    };
    D4.iter().map(|&turn| (0..long).filter(|&shift| leaves(turn, shift)).count() as u64).sum()
}

/// For each e: how long the vacuum's cycle of (R, e) is, how long its world, how many e give R
/// that world, and how many turns and shifts leave it as it is.
fn measures(r: &Table) -> Vec<(usize, usize, u64, u64)> {
    let worlds: Vec<(usize, Vec<Table>)> = (0..16u8).map(|e| world(r, e)).collect();
    let alike = |world: &Vec<Table>| worlds.iter().filter(|(_, other)| other == world).count() as u64;
    worlds.iter().map(|(cycle, world)| (*cycle, world.len(), alike(world), stabiliser(world))).collect()
}

/// What makes R special, in the order the special R are counted in: an affine direction, or a
/// turn or mirror (by its place in `D4`) and a block.
#[derive(Clone, Copy)]
enum Special {
    Affine(u8),
    Twisted(usize, u8),
}

fn specials() -> Vec<Special> {
    let mut all: Vec<Special> = (1..16u8).map(Special::Affine).collect();
    all.extend((1..D4.len()).flat_map(|turn| (0..16u8).map(move |v| Special::Twisted(turn, v))));
    all
}

fn is(r: &Table, special: Special) -> bool {
    match special {
        Special::Affine(w) => (0..16).all(|b| r[b ^ w as usize] == r[b] ^ r[w as usize]),
        Special::Twisted(turn, v) => {
            let turn = D4[turn];
            (0..16u8).all(|b| turn(r[b as usize]) == r[(turn(b) ^ v) as usize] ^ r[v as usize])
        }
    }
}

/// Every R with R(0) = 0 and R∘α = β∘R, handed to `found`. Each kind of special R is such a
/// table for some α and β: an affine direction w with R(w) = d makes them b ↦ b ⊕ w and
/// b ↦ b ⊕ d; a turn T with a block v and R(v) = r, b ↦ T(b) ⊕ v and b ↦ T(b) ⊕ r.
fn intertwining(alpha: &Table, beta: &Table, found: &mut dyn FnMut(&Table)) {
    const OPEN: u8 = 16;
    /// R(c) = x, and so along the cycles of α and β. False where that clashes.
    fn tie(
        r: &mut Table,
        used: &mut u16,
        pair: (&Table, &Table),
        (mut c, mut x): (u8, u8),
        tied: &mut Vec<u8>,
    ) -> bool {
        loop {
            if r[c as usize] != OPEN {
                return r[c as usize] == x;
            }
            if *used >> x & 1 == 1 {
                return false;
            }
            r[c as usize] = x;
            *used |= 1 << x;
            tied.push(c);
            (c, x) = (pair.0[c as usize], pair.1[x as usize]);
        }
    }
    fn fill(r: &mut Table, used: &mut u16, pair: (&Table, &Table), found: &mut dyn FnMut(&Table)) {
        let Some(c) = (0..16u8).find(|&c| r[c as usize] == OPEN) else {
            found(r);
            return;
        };
        for x in 0..16u8 {
            if *used >> x & 1 == 1 {
                continue;
            }
            let mut tied = Vec::new();
            if tie(r, used, pair, (c, x), &mut tied) {
                fill(r, used, pair, found);
            }
            for &c in &tied {
                *used &= !(1 << r[c as usize]);
                r[c as usize] = OPEN;
            }
        }
    }
    let (mut r, mut used) = ([OPEN; 16], 0u16);
    if tie(&mut r, &mut used, (alpha, beta), (0, 0), &mut Vec::new()) {
        fill(&mut r, &mut used, (alpha, beta), found);
    }
}

/// The work: each kind of special R with its α and β, once for every value of the block it
/// names.
fn work() -> Vec<(usize, Table, Table)> {
    let mut work = Vec::new();
    for (index, special) in specials().into_iter().enumerate() {
        match special {
            Special::Affine(w) => {
                for d in 1..16u8 {
                    work.push((index, std::array::from_fn(|b| b as u8 ^ w), std::array::from_fn(|b| b as u8 ^ d)));
                }
            }
            Special::Twisted(turn, v) => {
                let turn = D4[turn];
                for r in (0..16u8).filter(|r| (*r == 0) == (v == 0)) {
                    work.push((
                        index,
                        std::array::from_fn(|b| turn(b as u8) ^ v),
                        std::array::from_fn(|b| turn(b as u8) ^ r),
                    ));
                }
            }
        }
    }
    work
}

/// How many rules there are of the world of a rule, by going through them.
fn rules_of_its_world(rule: &BlockRule) -> u64 {
    let mut seen = HashSet::new();
    rule.each_in_world(|table| {
        seen.insert(*table);
        true
    });
    seen.len() as u64
}

#[test]
fn a_world_has_so_many_rules_as_its_length_and_symmetries_say() {
    let check = |r: &Table, e: u8, (_, long, alike, symmetries): (usize, usize, u64, u64)| {
        let rule = BlockRule::new(r.map(|outcome| outcome ^ e)).unwrap();
        assert_eq!(rules_of_its_world(&rule) * symmetries, 8 * long as u64 * alike, "{rule}");
    };
    // Rules at random, nearly none of them special.
    let mut rng = Rng::new(9);
    for _ in 0..3000 {
        let rule = BlockRule::random(|| rng.next_u64());
        let e = rule.table()[0];
        let r = rule.table().map(|outcome| outcome ^ e);
        check(&r, e, measures(&r)[e as usize]);
    }
    // The first few special rules of every kind.
    for (_, alpha, beta) in work() {
        let mut taken = 0;
        intertwining(&alpha, &beta, &mut |r| {
            if taken < 3 {
                taken += 1;
                for (e, measured) in measures(r).into_iter().enumerate() {
                    check(r, e as u8, measured);
                }
            }
        });
    }
}

#[test]
#[ignore = "takes half an hour on a dozen cores"]
fn every_rule_there_is_makes_so_many_worlds() {
    let specials = specials();
    // How many special rules have a vacuum's cycle of each length, and what their worlds of
    // each length count for, over a common denominator.
    let lcm = 720_720i128;
    let denominator = 8 * lcm * lcm;
    let tally = work()
        .par_iter()
        .map(|(index, alpha, beta)| {
            let (mut cycles, mut worlds) = ([0u64; 17], [0i128; 17]);
            intertwining(alpha, beta, &mut |r| {
                // Each special R is counted with the first kind it is of.
                if specials[..*index].iter().any(|&special| is(r, special)) {
                    return;
                }
                for (cycle, long, alike, symmetries) in measures(r) {
                    cycles[cycle] += 1;
                    worlds[long] += symmetries as i128 * (denominator / (8 * long as i128 * alike as i128));
                }
            });
            (cycles, worlds)
        })
        .reduce(
            || ([0u64; 17], [0i128; 17]),
            |(mut cycles, mut worlds), (more_cycles, more_worlds)| {
                for long in 0..17 {
                    cycles[long] += more_cycles[long];
                    worlds[long] += more_worlds[long];
                }
                (cycles, worlds)
            },
        );
    let (cycles, worlds) = tally;
    // Of every length of cycle there are 15! rules, and those that are not special make worlds
    // of that length, 8·L rules to a world.
    let factorial: i128 = (1..=15).product();
    let mut total = 0;
    for long in 1..=16 {
        let ordinary = (factorial - cycles[long] as i128) * (denominator / (8 * long as i128));
        let numerator = ordinary + worlds[long];
        assert_eq!(numerator % denominator, 0, "the worlds of {long} tables come to a whole number");
        total += numerator / denominator;
        if long == 1 {
            // A world of one table has a rule that leaves the empty world empty.
            let stable = Family::parse("stable-vacuum").unwrap().size(4, u64::MAX, &Progress::default());
            assert_eq!(Some(numerator / denominator), stable.and_then(|size| size.canonical).map(i128::from));
        }
    }
    assert_eq!(Some(total), EVERY_RULE.canonical.map(i128::from));
}
