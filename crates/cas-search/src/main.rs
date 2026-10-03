//! cas-search — looks for interesting rules: puts every rule of a family through the trials of
//! `cas_core::search` and writes one line per rule.

use std::{
    cmp::Reverse,
    collections::HashSet,
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use cas_core::{
    families::{self, ENUMERABLE, Family},
    rules::{BlockRule, Population},
    search::{self, Effort, Report},
    universe::Rng,
};
use clap::{
    Arg, Command, Parser,
    builder::{PossibleValue, TypedValueParser},
    error::ErrorKind,
};

/// Looks for interesting reversible block cellular automata.
#[derive(Parser, Debug)]
#[command(name = "cas-search", version, about)]
struct Args {
    /// The rules to measure: those with all of the properties named, joined by `+`, as in
    /// `mirror+conserving`.
    ///
    /// Rules that make the same world are measured once. A family of more than eight million
    /// rules is not gone through but sampled: `--limit` rules of it, drawn with `--seed`.
    #[arg(long, value_parser = FamilyParser, default_value = "quarter-turn")]
    family: Vec<Family>,
    /// Measure these rules instead of a family: presets, ESPCA numbers or tables.
    #[arg(long = "rule")]
    rules: Vec<BlockRule>,
    /// Measure the rules of a table that an earlier search wrote instead of a family, its best
    /// first. With --limit and more seeds or generations: a closer look at so many of its best.
    #[arg(long)]
    from: Option<PathBuf>,
    /// The seed the rules of a family too big to go through are drawn with.
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
    /// so a search that was interrupted is taken up where it stopped. The file says in its
    /// first line how hard its rules were looked at, and takes no rules looked at otherwise.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Measure at most this many rules now. Those of a family are taken in a shuffled order,
    /// so that a part of it is a fair sample; of a family too big to go through, so many are
    /// drawn, 1000 unless said.
    #[arg(long)]
    limit: Option<usize>,
    /// How many of the best rules to print at the end.
    #[arg(long, default_value_t = 20)]
    top: usize,
    /// Threads to search on.
    #[arg(long, default_value_t = default_threads())]
    threads: usize,
}

/// So many rules are drawn of a family too big to go through, unless `--limit` says.
const DRAWN: usize = 1000;

/// Reads a family, and tells the help what may be written for one.
#[derive(Clone)]
struct FamilyParser;

impl TypedValueParser for FamilyParser {
    type Value = Family;

    fn parse_ref(&self, command: &Command, _: Option<&Arg>, value: &OsStr) -> Result<Family, clap::Error> {
        let family = value.to_str().ok_or("not text".to_string()).and_then(Family::parse);
        family.map_err(|error| clap::Error::raw(ErrorKind::InvalidValue, format!("{error}\n")).with_cmd(command))
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        let values = families::catalogue().into_iter().map(|(name, about)| PossibleValue::new(name).help(about));
        Some(Box::new(values))
    }
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

/// The line a table begins with: how hard its rules were looked at. Kinds and periods are
/// counts that grow with the effort, so rows measured unalike do not belong in one table.
fn measured_with(effort: &Effort) -> String {
    format!("# measured with --seeds {} --generations {} --blob {}", effort.seeds, effort.generations, effort.blob)
}

/// A table written earlier: the line that says how its rules were measured, if it has one,
/// and its lines. Neither if there is no such file.
fn table(path: &Path) -> Result<(Option<String>, Vec<Vec<String>>), String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok((None, Vec::new()));
    };
    let (notes, lines): (Vec<&str>, Vec<&str>) = text.lines().partition(|line| line.starts_with('#'));
    let split = |line: &str| line.split('\t').map(str::to_string).collect::<Vec<_>>();
    let mut lines = lines.into_iter().map(split);
    if lines.next().is_some_and(|header| header != COLUMNS) {
        return Err(format!("{} has other columns than this version writes: name another file", path.display()));
    }
    let measured = notes.into_iter().find(|note| note.starts_with("# measured with")).map(str::to_string);
    Ok((measured, lines.filter(|line| line.len() == COLUMNS.len()).collect()))
}

/// Opens a table to add to, with the lines it has already. A new one is made, with the folder
/// it is to lie in if need be, and a new or empty one gets its first two lines: how its rules
/// are measured, and the names of the columns. A table measured otherwise is not added to.
fn open(path: &Path, effort: &Effort) -> Result<(File, Vec<Vec<String>>), String> {
    let (then, lines) = table(path)?;
    let now = measured_with(effort);
    let flags = |line: &str| line.trim_start_matches("# measured with ").to_string();
    match then {
        Some(then) if then != now => {
            return Err(format!(
                "the rules in {} were measured with {}, and these would be with {}: name another file, or measure alike",
                path.display(),
                flags(&then),
                flags(&now)
            ));
        }
        None if !lines.is_empty() => {
            eprintln!("{} does not say how its rules were measured: taken to be as now", path.display());
        }
        _ => {}
    }
    let cannot = |error: std::io::Error| format!("cannot write {}: {error}", path.display());
    if let Some(folder) = path.parent().filter(|folder| !folder.as_os_str().is_empty()) {
        std::fs::create_dir_all(folder).map_err(cannot)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(cannot)?;
    if file.metadata().map_err(cannot)?.len() == 0 {
        writeln!(file, "{now}\n{}", COLUMNS.join("\t")).map_err(cannot)?;
    }
    Ok((file, lines))
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
    // The table first: if it cannot be written, or holds rules measured otherwise, there is
    // no point in going through a family. Lines of an earlier run count as done.
    let (file, mut lines) = match &args.out {
        Some(path) => {
            let (file, lines) = open(path, &effort).unwrap_or_else(|error| fail(&error));
            (Some(file), lines)
        }
        None => (None, Vec::new()),
    };
    let chosen = !args.rules.is_empty() || args.from.is_some();
    let rules = if !args.rules.is_empty() {
        args.rules.clone()
    } else if let Some(path) = &args.from {
        let (_, mut lines) = table(path).unwrap_or_else(|error| fail(&error));
        if lines.is_empty() {
            fail(&format!("no table of rules in {}", path.display()));
        }
        lines.sort_by_key(|line| Reverse(merit(line)));
        // The best so many of the table, whichever of them an earlier run got to.
        lines.truncate(args.limit.unwrap_or(lines.len()));
        let rule = |line: &Vec<String>| line[0].parse().unwrap_or_else(|error: String| fail(&error));
        lines.iter().map(rule).collect()
    } else {
        let family = Family::new(args.family.iter().flat_map(|family| family.constraints().iter().copied()));
        let named = match family.constraints() {
            [] => "every rule there is".to_string(),
            constraints => constraints.iter().map(|constraint| constraint.to_string()).collect::<Vec<_>>().join("+"),
        };
        let rules = match family.count(ENUMERABLE) {
            Some(count) => {
                eprintln!("{named}: {count} rules");
                family.rules()
            }
            None => {
                let drawn = args.limit.unwrap_or(DRAWN);
                eprintln!("{named}: too many rules to go through, {drawn} drawn with seed {}", args.seed);
                family.sample(drawn, args.seed)
            }
        };
        families::distinct(rules)
    };

    let done: HashSet<&str> = lines.iter().map(|line| line[0].as_str()).collect();
    let mut todo: Vec<BlockRule> = rules.into_iter().filter(|rule| !done.contains(rule.to_string().as_str())).collect();
    let left = todo.len();
    if !chosen {
        shuffle(&mut todo);
    }
    todo.truncate(args.limit.unwrap_or(left));
    eprintln!("{} rules to measure of {left}, {} already in the table", todo.len(), lines.len());

    let started = Instant::now();
    // The table, the lines of this run, and when progress was last reported.
    let progress = Mutex::new((file, Vec::new(), started));
    search::survey(&todo, &effort, |rule, report| {
        let line = line(rule, &report);
        let mut progress = progress.lock().unwrap();
        let (file, measured, reported) = &mut *progress;
        if let Some(file) = file {
            // One write for the whole line: a search that is cut short leaves no half lines.
            // A line that cannot be written ends the search: its rules would be lost.
            if let Err(error) = file.write_all(format!("{}\n", line.join("\t")).as_bytes()) {
                fail(&format!("cannot write the table: {error}"));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A place for the tables of one test.
    fn folder(test: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("cas-search-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn row(rule: &str) -> String {
        let mut cells = vec![String::new(); COLUMNS.len()];
        cells[0] = rule.to_string();
        cells.join("\t")
    }

    #[test]
    fn a_table_says_how_it_was_measured_and_takes_nothing_else() {
        let folder = folder("measured");
        // The folder is made along with the table.
        let path = folder.join("deep").join("table.tsv");
        let effort = Effort::default();
        let (mut file, lines) = open(&path, &effort).unwrap();
        assert!(lines.is_empty());
        writeln!(file, "{}", row("a rule")).unwrap();
        drop(file);
        let text = std::fs::read_to_string(&path).unwrap();
        let first: Vec<&str> = text.lines().take(2).collect();
        assert_eq!(first, ["# measured with --seeds 400 --generations 3000 --blob 8000", &COLUMNS.join("\t")]);
        // Taken up again, it has its line, and no second heading.
        let (file, lines) = open(&path, &effort).unwrap();
        drop(file);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0][0], "a rule");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        // Rules looked at harder belong in another table.
        let closer = Effort { seeds: 1600, ..Effort::default() };
        let refusal = open(&path, &closer).unwrap_err();
        assert!(refusal.contains("--seeds 400") && refusal.contains("--seeds 1600"), "{refusal}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_empty_file_and_an_older_table_are_taken_up() {
        let folder = folder("older");
        std::fs::create_dir_all(&folder).unwrap();
        // An empty file is a table yet to be begun.
        let empty = folder.join("empty.tsv");
        std::fs::write(&empty, "").unwrap();
        drop(open(&empty, &Effort::default()).unwrap());
        assert_eq!(std::fs::read_to_string(&empty).unwrap().lines().count(), 2);
        // A table from before the first line was written has its columns and its rows.
        let older = folder.join("older.tsv");
        std::fs::write(&older, format!("{}\n{}\n", COLUMNS.join("\t"), row("an old rule"))).unwrap();
        let (file, lines) = open(&older, &Effort::default()).unwrap();
        drop(file);
        assert_eq!(lines.len(), 1);
        // Other columns are another version's table.
        let other = folder.join("other.tsv");
        std::fs::write(&other, "rule\tsomething\n").unwrap();
        assert!(open(&other, &Effort::default()).unwrap_err().contains("other columns"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn worlds_with_things_that_travel_and_things_that_stay_come_first() {
        let line = |character: &str, spaceships: u64, periods: u64| {
            let mut cells = vec![String::new(); COLUMNS.len()];
            cells[column("character")] = character.to_string();
            cells[column("spaceships")] = spaceships.to_string();
            cells[column("periods")] = periods.to_string();
            cells
        };
        // By the lesser of kinds and periods; then guns by their kinds; the rest is no find.
        assert!(merit(&line("spaceships", 8, 40)) > merit(&line("spaceships", 30, 2)));
        assert!(merit(&line("spaceships", 1, 1)) > merit(&line("linear", 9, 0)));
        assert!(merit(&line("linear", 2, 0)) > merit(&line("linear", 1, 5)));
        assert_eq!(merit(&line("linear", 0, 3)), merit(&line("explosive", 5, 5)));
    }
}
