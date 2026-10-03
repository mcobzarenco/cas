//! cas-search — looks for interesting rules: puts every rule of a family through the trials of
//! `cas_core::search` and writes one line per rule.

use std::{
    cmp::Reverse,
    collections::HashSet,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use cas_core::{
    families,
    rules::{BlockRule, Population, mirror, rotate_180, rotate_cw},
    search::{self, Effort, Report},
    universe::Rng,
};
use clap::{Parser, ValueEnum};

/// Looks for interesting reversible block cellular automata.
#[derive(Parser, Debug)]
#[command(name = "cas-search", version, about)]
struct Args {
    /// The rules to measure. Rules that differ only by a turn or a mirror are measured once.
    #[arg(long, value_enum, default_value = "quarter-turn")]
    family: Family,
    /// Measure these rules instead of a family: presets, ESPCA numbers or tables.
    #[arg(long = "rule")]
    rules: Vec<BlockRule>,
    /// Measure the rules of a table that an earlier search wrote instead of a family, its best
    /// first. With --limit and more seeds or generations: a closer look at the best of it.
    #[arg(long)]
    from: Option<PathBuf>,
    /// How many random rules to draw.
    #[arg(long, default_value_t = 1000)]
    count: usize,
    /// The seed the random rules are drawn with.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Small random patterns each rule is tried on.
    #[arg(long, default_value_t = Effort::default().seeds)]
    seeds: usize,
    /// Generations each of them is followed for.
    #[arg(long, default_value_t = Effort::default().generations)]
    generations: u32,
    /// Generations a blob is left alone, on a closed grid and with the border open; 0 skips it.
    #[arg(long, default_value_t = Effort::default().blob)]
    blob: i64,
    /// Write one tab-separated line per rule to this file. Rules already in it are skipped,
    /// so a search that was interrupted is taken up where it stopped.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Measure at most this many rules now. Those of a family are taken in a shuffled order,
    /// so that a part of it is a fair sample.
    #[arg(long)]
    limit: Option<usize>,
    /// How many of the best rules to print at the end.
    #[arg(long, default_value_t = 20)]
    top: usize,
    /// Threads to search on.
    #[arg(long, default_value_t = default_threads())]
    threads: usize,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Family {
    /// The 1536 rules that look the same after a quarter turn: Morita's ESPCAs.
    QuarterTurn,
    /// The 1 105 920 rules that look the same after a half turn.
    HalfTurn,
    /// The 1 105 920 rules that look the same in a mirror.
    Mirror,
    /// The 829 440 rules that keep the number of cells of every block, or trade it for the
    /// number of its dead cells as Critters does: patterns keep their number of cells.
    Conserving,
    /// The rules that keep a weighted number of cells, some cells counting for several, and
    /// not their number: cells are made and unmade, yet nothing can explode.
    Weighted,
    /// Random permutations of the sixteen blocks, `--count` of them.
    Random,
}

fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).max(1)
}

const COLUMNS: [&str; 19] = [
    "rule", "espca", "character", "cells", "vacuum", "oscillating", "travelling", "scattering",
    "growing", "undecided", "growth", "spaceships", "periods", "longest", "damage", "blob",
    "remaining", "caught", "others",
];

fn column(name: &str) -> usize {
    COLUMNS.iter().position(|&column| column == name).expect("a column of the table")
}

/// A line of the table. Shares of the seeds are in percent, as is the damage; what was not
/// tried is left empty.
fn line(rule: &BlockRule, report: &Report) -> Vec<String> {
    let percent = |share: f32| format!("{:.1}", 100.0 * share);
    let cells = match rule.population() {
        Population::Conserved => "conserved".to_string(),
        Population::ConservedRelativeToVacuum => "conserved relative to the vacuum".to_string(),
        Population::Weighted(weights) => format!("conserved by weight {}", Population::weights_text(&weights)),
        Population::NotConserved => "not conserved".to_string(),
    };
    let multiple = |cells: Option<f32>| cells.map(|cells| format!("{cells:.2}")).unwrap_or_default();
    let left = |count: u64| report.remaining.map(|_| count.to_string()).unwrap_or_default();
    vec![
        rule.to_string(),
        rule.espca().unwrap_or_default(),
        format!("{:?}", report.character()).to_lowercase(),
        cells,
        rule.vacuum_cycle().len().to_string(),
        percent(report.oscillating),
        percent(report.travelling),
        percent(report.scattering),
        percent(report.growing),
        percent(report.undecided),
        format!("{:.2}", report.growth),
        report.spaceships.to_string(),
        report.periods.to_string(),
        report.longest_period.to_string(),
        format!("{:.2}", 100.0 * report.damage),
        multiple(report.blob),
        multiple(report.remaining),
        left(report.caught),
        left(report.others),
    ]
}

/// The lines of a table written earlier; none if there is no such file.
fn table(path: &Path) -> Vec<Vec<String>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let split = |line: &str| line.split('\t').map(str::to_string).collect::<Vec<_>>();
    let mut lines = text.lines().map(split);
    if lines.next().is_some_and(|header| header != COLUMNS) {
        fail(&format!("{} has other columns than this version writes: name another file", path.display()));
    }
    lines.filter(|line| line.len() == COLUMNS.len()).collect()
}

/// What makes a rule a find, the more the better. First the worlds with things that travel
/// and things that stay: by the lesser of their kinds of spaceship and their periods (by
/// kinds alone, rules in which a lone cell already flies would lead). Then the rules whose
/// seeds grow along lines and send out spaceships, by the kinds they send. What explodes,
/// ignites or stands still is no find.
fn merit(line: &[String]) -> (u8, u64, u64, u64) {
    let number = |name: &str| line[column(name)].parse::<u64>().unwrap_or(0);
    let (spaceships, periods) = (number("spaceships"), number("periods"));
    match line[column("character")].as_str() {
        "spaceships" => (2, spaceships.min(periods), spaceships, periods),
        "linear" if spaceships > 0 => (1, spaceships, periods, 0),
        _ => (0, 0, 0, 0),
    }
}

fn main() {
    let args = Args::parse();
    cas_core::use_threads(args.threads);
    let effort = Effort {
        seeds: args.seeds,
        generations: args.generations,
        blob: args.blob,
    };
    let chosen = !args.rules.is_empty() || args.from.is_some();
    let rules = if !args.rules.is_empty() {
        args.rules.clone()
    } else if let Some(path) = &args.from {
        let mut lines = table(path);
        if lines.is_empty() {
            fail(&format!("no table of rules in {}", path.display()));
        }
        lines.sort_by_key(|line| Reverse(merit(line)));
        let rule = |line: &Vec<String>| line[0].parse().unwrap_or_else(|error: String| fail(&error));
        lines.iter().map(rule).collect()
    } else {
        families::distinct(match args.family {
            Family::QuarterTurn => families::symmetric_under(rotate_cw),
            Family::HalfTurn => families::symmetric_under(rotate_180),
            Family::Mirror => families::symmetric_under(mirror),
            Family::Conserving => families::conserving(),
            Family::Weighted => families::weighted(),
            Family::Random => families::random(args.count, args.seed),
        })
    };

    // Lines of an earlier run count as done.
    let mut lines = args.out.as_deref().map(table).unwrap_or_default();
    let done: HashSet<&str> = lines.iter().map(|line| line[0].as_str()).collect();
    let mut todo: Vec<BlockRule> = rules.into_iter().filter(|rule| !done.contains(rule.to_string().as_str())).collect();
    let left = todo.len();
    if !chosen {
        shuffle(&mut todo);
    }
    todo.truncate(args.limit.unwrap_or(left));
    eprintln!("{} rules to measure of {left}, {} already in the table", todo.len(), lines.len());

    let file = args.out.as_ref().map(|path| {
        let fresh = !path.exists();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|error| fail(&format!("cannot write {}: {error}", path.display())));
        if fresh {
            writeln!(file, "{}", COLUMNS.join("\t")).unwrap_or_else(|error| fail(&error.to_string()));
        }
        file
    });

    let started = Instant::now();
    // The table, the lines of this run, and when progress was last reported.
    let progress = Mutex::new((file, Vec::new(), started));
    search::survey(&todo, &effort, |rule, report| {
        let line = line(rule, &report);
        let mut progress = progress.lock().unwrap();
        let (file, measured, reported) = &mut *progress;
        if let Some(file) = file {
            // One write for the whole line: a search that is cut short leaves no half lines.
            let _ = file.write_all(format!("{}\n", line.join("\t")).as_bytes());
        }
        measured.push(line);
        if reported.elapsed() >= Duration::from_secs(2) {
            *reported = Instant::now();
            let elapsed = started.elapsed().as_secs_f64();
            let left = elapsed / measured.len() as f64 * (todo.len() - measured.len()) as f64;
            eprintln!("{} of {} measured, about {} to go", measured.len(), todo.len(), clock(left));
        }
    });
    let (_, measured, _) = progress.into_inner().unwrap();
    eprintln!("{} rules measured in {}", measured.len(), clock(started.elapsed().as_secs_f64()));
    // Rules asked for by name, and not for a table: all there is to say about each.
    if !args.rules.is_empty() && args.out.is_none() {
        for line in &measured {
            let cells = COLUMNS.iter().zip(line).map(|(name, cell)| format!("{name:<12}{cell}"));
            println!("{}\n", cells.collect::<Vec<_>>().join("\n"));
        }
        return;
    }
    lines.extend(measured);

    let characters = ["explosive", "growing", "linear", "igniting", "spaceships", "gas", "confined", "frozen", "other"];
    let count = |character: &str| lines.iter().filter(|line| line[column("character")] == character).count();
    let counts: Vec<String> = characters.iter().map(|character| format!("{} {character}", count(character))).collect();
    println!("{} rules: {}", lines.len(), counts.join(", "));

    lines.sort_by_key(|line| Reverse(merit(line)));
    let best = |kind: u8| lines.iter().filter(move |line| merit(line).0 == kind).take(args.top).collect::<Vec<_>>();
    show(
        "The best worlds, with things that travel and things that stay:",
        &best(2),
        &["rule", "espca", "spaceships", "periods", "longest", "damage", "blob", "remaining", "cells"],
    );
    show(
        "The best of the rules whose seeds grow along lines, by the kinds of spaceship they send out:",
        &best(1),
        &["rule", "espca", "growing", "growth", "spaceships", "blob"],
    );
}

/// Prints some columns of some lines, under a title; nothing if there are no lines.
fn show(title: &str, lines: &[&Vec<String>], columns: &[&str]) {
    if lines.is_empty() {
        return;
    }
    let cell = |line: &Vec<String>, name: &str| line[column(name)].clone();
    let width = |name: &str| lines.iter().map(|line| cell(line, name).len()).max().unwrap_or(0).max(name.len());
    let print = |cells: Vec<String>| {
        let padded: Vec<String> = cells.iter().zip(columns).map(|(cell, name)| format!("{cell:<width$}", width = width(name))).collect();
        println!("{}", padded.join("  ").trim_end());
    };
    println!("\n{title}");
    print(columns.iter().map(|name| name.to_string()).collect());
    for line in lines {
        print(columns.iter().map(|name| cell(line, name)).collect());
    }
}

/// Always the same shuffle: what is left of a family after some runs does not depend on how
/// the runs were cut.
fn shuffle(rules: &mut [BlockRule]) {
    let mut rng = Rng::new(0);
    for i in (1..rules.len()).rev() {
        rules.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
    }
}

/// Seconds as `h:mm:ss`.
fn clock(seconds: f64) -> String {
    let seconds = seconds.round() as u64;
    format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
}

fn fail(message: &str) -> ! {
    eprintln!("cas-search: {message}");
    std::process::exit(2)
}
