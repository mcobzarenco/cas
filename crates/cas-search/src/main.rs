//! cas-search — looks for interesting rules: puts every rule of a family through the trials of
//! `cas_core::search` and writes one line per rule.

use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use cas_core::{
    rules::{BlockRule, Population},
    search::{self, Effort, Report},
    universe::Rng,
};
use clap::{Parser, ValueEnum};

/// Looks for interesting reversible block cellular automata.
#[derive(Parser, Debug)]
#[command(name = "cas-search", version, about)]
struct Args {
    /// The rules to measure. Rules that differ only by a turn or a mirror are measured once.
    #[arg(long, value_enum, default_value = "espca")]
    family: Family,
    /// Measure these rules instead of a family: presets, ESPCA numbers or tables.
    #[arg(long = "rule")]
    rules: Vec<BlockRule>,
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
    /// Generations a blob is left to evaporate through an open border; 0 skips that trial.
    #[arg(long, default_value_t = Effort::default().evaporation)]
    evaporation: i64,
    /// Write one tab-separated line per rule to this file. Rules already in it are skipped,
    /// so a search that was interrupted is taken up where it stopped.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Measure at most this many rules now. They are taken in a shuffled order, so that a
    /// part of a family is a fair sample of it.
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
    Espca,
    /// The 829 440 rules that keep the number of cells of every block, or trade it for the
    /// number of its dead cells as Critters does: patterns keep their number of cells.
    Conserving,
    /// Random permutations of the sixteen blocks, `--count` of them.
    Random,
}

fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).max(1)
}

const COLUMNS: [&str; 17] = [
    "rule", "espca", "character", "cells", "vacuum", "oscillating", "travelling", "scattering",
    "growing", "undecided", "spaceships", "periods", "longest", "damage", "remaining", "caught",
    "others",
];
/// Where in a line the columns are that the best rules are chosen by.
const SPACESHIPS: usize = 10;
const PERIODS: usize = 11;

/// A line of the table: shares are in percent.
fn line(rule: &BlockRule, report: &Report) -> Vec<String> {
    let percent = |share: f32| format!("{:.1}", 100.0 * share);
    let cells = match rule.population() {
        Population::Conserved => "conserved",
        Population::ConservedRelativeToVacuum => "conserved relative to the vacuum",
        Population::NotConserved => "not conserved",
    };
    vec![
        rule.to_string(),
        rule.espca().unwrap_or_default(),
        format!("{:?}", report.character()).to_lowercase(),
        cells.to_string(),
        rule.vacuum_cycle().len().to_string(),
        percent(report.oscillating),
        percent(report.travelling),
        percent(report.scattering),
        percent(report.growing),
        percent(report.undecided),
        report.spaceships.to_string(),
        report.periods.to_string(),
        report.longest_period.to_string(),
        format!("{:.2}", 100.0 * report.damage),
        percent(report.remaining),
        report.caught.to_string(),
        report.others.to_string(),
    ]
}

fn main() {
    let args = Args::parse();
    cas_core::use_threads(args.threads);
    let effort = Effort {
        seeds: args.seeds,
        generations: args.generations,
        evaporation: args.evaporation,
    };
    let rules = if !args.rules.is_empty() {
        args.rules.clone()
    } else {
        search::distinct(match args.family {
            Family::Espca => search::rotation_symmetric(),
            Family::Conserving => search::conserving(),
            Family::Random => search::random(args.count, args.seed),
        })
    };

    // Lines of an earlier run count as done.
    let mut lines: Vec<Vec<String>> = Vec::new();
    if let Some(path) = &args.out
        && let Ok(text) = std::fs::read_to_string(path)
    {
        let split = |line: &str| line.split('\t').map(str::to_string).collect::<Vec<_>>();
        lines = text.lines().skip(1).map(split).filter(|line| line.len() == COLUMNS.len()).collect();
    }
    let done: HashSet<&str> = lines.iter().map(|line| line[0].as_str()).collect();
    let mut todo: Vec<BlockRule> = rules.into_iter().filter(|rule| !done.contains(rule.to_string().as_str())).collect();
    let left = todo.len();
    shuffle(&mut todo);
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
    lines.extend(measured);

    // The best have things that travel and things that stay: by the lesser of their kinds of
    // spaceship and their periods. By kinds alone, rules in which a lone cell flies would lead.
    let number = |line: &Vec<String>, column: usize| line[column].parse::<u64>().unwrap_or(0);
    lines.sort_by_key(|line| {
        let (spaceships, periods) = (number(line, SPACESHIPS), number(line, PERIODS));
        std::cmp::Reverse((spaceships.min(periods), spaceships, periods))
    });
    let shown = [0, 1, 2, 10, 11, 12, 13];
    let width = |column: usize| {
        let longest = lines.iter().take(args.top).map(|line| line[column].len()).max().unwrap_or(0);
        longest.max(COLUMNS[column].len())
    };
    let widths: Vec<usize> = shown.iter().map(|&column| width(column)).collect();
    let print = |cells: Vec<&str>| {
        let padded: Vec<String> = cells.iter().zip(&widths).map(|(cell, &width)| format!("{cell:<width$}")).collect();
        println!("{}", padded.join("  ").trim_end());
    };
    print(shown.iter().map(|&column| COLUMNS[column]).collect());
    for line in lines.iter().take(args.top) {
        print(shown.iter().map(|&column| line[column].as_str()).collect());
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
