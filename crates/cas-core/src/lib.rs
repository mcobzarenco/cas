//! Reversible block cellular automata, without the app: the rules, the grid and its stepping,
//! patterns and what they do when left alone, counting the spaceships a rule produces,
//! measuring the rules of a family to find the interesting ones, the library of the rules
//! that were kept, and the collection of the patterns that were.

pub mod census;
pub mod collection;
pub mod families;
pub mod library;
pub mod pattern;
pub mod rules;
pub mod search;
pub mod universe;

/// How many threads share out the stepping of large grids and the measuring of rules. To be
/// called once, before either; without it there are as many as the machine has.
pub fn use_threads(threads: usize) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build_global()
        .expect("nothing has used the thread pool yet");
}
