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
    families::{self, COUNTABLE, Family, Progress},
    library::{Library, usual_file},
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
    /// Rules that make the same world are measured once: the family's canonical rules, all of
    /// them unless `--sample` is given. A family of more than 250 million rules is too big to
    /// go through, and can only be sampled.
    #[arg(long, value_parser = FamilyParser, default_value = "quarter-turn")]
    family: Vec<Family>,
    /// Measure these rules instead of a family: presets, ESPCA numbers or tables.
    #[arg(long = "rule")]
    rules: Vec<BlockRule>,
    /// Measure the rules of a table that an earlier search wrote instead of a family, its best
    /// first. With --limit and more seeds or generations: a closer look at so many of its best.
    #[arg(long)]
    from: Option<PathBuf>,
    /// The seed the rules of a sample are drawn with, of a family too big to go through.
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
    /// Measure at most this many rules now. A family with more of them left to measure is not
    /// measured at all, unless `--sample` is given; of a table (`--from`), the best so many
    /// are measured.
    #[arg(long)]
    limit: Option<usize>,
    /// Measure `--limit` rules of the family, picked at random, rather than all of it: its
    /// canonical rules are taken in a shuffled order, the same every time; of a family too big
    /// to go through, rules are drawn with `--seed` until so many canonical ones are found.
    #[arg(long, requires = "limit")]
    sample: bool,
    /// How many of the best rules to print at the end.
    #[arg(long, default_value_t = 20)]
    top: usize,
    /// Keep so many of the best rules in the rule library of the app, each named after what
    /// was searched and numbered, with what was measured as its note. A rule whose world the
    /// library has already is passed over.
    #[arg(long, default_value_t = 0)]
    keep: usize,
    /// The file of the rule library, for --keep: `rules.tsv` of the repository the program
    /// was built from, which is the app's, unless another is named.
    #[arg(long)]
    library: Option<PathBuf>,
    /// Threads to search on.
    #[arg(long, default_value_t = default_threads())]
    threads: usize,
}

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
    "rule",
    "espca",
    "character",
    "cells",
    "vacuum",
    "oscillating",
    "travelling",
    "scattering",
    "growing",
    "undecided",
    "growth",
    "spaceships",
    "periods",
    "longest",
    "damage",
    "blob",
    "remaining",
    "caught",
    "others",
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
    let effort = Effort { seeds: args.seeds, generations: args.generations, blob: args.blob };
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
    if chosen && args.sample {
        fail("--sample picks rules of a family; --rule and --from name them");
    }
    let done: HashSet<String> = lines.iter().map(|line| line[0].clone()).collect();
    let (todo, left) = if chosen {
        let rules = if !args.rules.is_empty() {
            args.rules.clone()
        } else {
            let path = args.from.as_ref().expect("a table is named");
            let (_, mut lines) = table(path).unwrap_or_else(|error| fail(&error));
            if lines.is_empty() {
                fail(&format!("no table of rules in {}", path.display()));
            }
            lines.sort_by_key(|line| Reverse(merit(line)));
            // The best so many of the table, whichever of them an earlier run got to.
            lines.truncate(args.limit.unwrap_or(lines.len()));
            let rule = |line: &Vec<String>| line[0].parse().unwrap_or_else(|error: String| fail(&error));
            lines.iter().map(rule).collect()
        };
        let mut todo: Vec<BlockRule> = rules.into_iter().filter(|rule| !done.contains(&rule.to_string())).collect();
        let left = todo.len();
        todo.truncate(args.limit.unwrap_or(left));
        (todo, left)
    } else {
        let (said, todo, left) = of_family(&args, &done, COUNTABLE).unwrap_or_else(|error| fail(&error));
        eprintln!("{said}");
        (todo, left)
    };
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
        let mut measured = measured;
        measured.sort_by_key(|line| Reverse(merit(line)));
        store(&args, &measured);
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
    // A list of nothing is not printed: with no list at all, that is said.
    if args.top > 0 && !lines.is_empty() && lines.iter().all(|line| merit(line).0 == 0) {
        let table = match &args.out {
            Some(path) => format!("What was measured of each rule is in {}.", path.display()),
            None => "With --out FILE, what was measured of each rule is written to a table.".to_string(),
        };
        println!(
            "\nNone of these rules is a find (a world where something travels slower than light, or a rule\n\
             whose seeds grow along lines and send spaceships out), so there is no list of the best.\n{table}"
        );
    }

    if args.keep > 0 {
        println!();
    }
    store(&args, &lines);
}

/// Keeps the best of the rules, which come best first, in the file of the rule library, if
/// that was asked for, and says what became of each.
fn store(args: &Args, lines: &[Vec<String>]) {
    if args.keep == 0 {
        return;
    }
    // What was searched names what is kept: the table the rules are from, or the family. Rules
    // that were given go by the name the app gives one kept without a name.
    let searched = match (args.rules.is_empty(), &args.from) {
        (false, _) => "Unnamed".to_string(),
        (true, Some(path)) => path.file_stem().map_or("search".to_string(), |stem| stem.to_string_lossy().to_string()),
        (true, None) => {
            let asked = args.family.iter().flat_map(|family| family.constraints());
            match asked.map(|constraint| constraint.to_string()).collect::<Vec<_>>() {
                names if names.is_empty() => "random".to_string(),
                names => names.join("+"),
            }
        }
    };
    let file = args.library.clone().unwrap_or_else(usual_file);
    let read = Library::read(&file).unwrap_or_else(|error| fail(&format!("{}: {error}", file.display())));
    let mut library = read.clone();
    let said = keep(&mut library, lines, args.keep, &searched);
    if said.is_empty() {
        println!("nothing to keep: none of the rules is a find");
    }
    for said in said {
        println!("{said}");
    }
    // A file nothing was added to is left as it is.
    if library != read {
        library.write(&file).unwrap_or_else(|error| fail(&format!("cannot write {}: {error}", file.display())));
    }
}

/// Keeps the best of the rules, which come best first, in the library: so many at most, and
/// only finds. Says what became of each.
fn keep(library: &mut Library, lines: &[Vec<String>], most: usize, searched: &str) -> Vec<String> {
    let finds = lines.iter().filter(|line| merit(line).0 > 0).take(most);
    let cell = |line: &[String], name: &str| line[column(name)].clone();
    let counted = |count: String, one: &str, many: &str| format!("{count} {}", if count == "1" { one } else { many });
    let mut said = Vec::new();
    for line in finds {
        let Ok(rule) = cell(line, "rule").parse::<BlockRule>() else {
            continue;
        };
        let name = library.unused(searched);
        match library.keep(rule, &name) {
            Ok(kept) => {
                library.tag(kept, "search");
                library.annotate(
                    kept,
                    &format!(
                        "{}: {}, {}, the longest {}",
                        cell(line, "character"),
                        counted(cell(line, "spaceships"), "kind of spaceship", "kinds of spaceship"),
                        counted(cell(line, "periods"), "period", "periods"),
                        cell(line, "longest")
                    ),
                );
                said.push(format!("kept {} as “{name}”", cell(line, "rule")));
            }
            Err(known) => {
                said.push(format!(
                    "{} is in the library already, as “{}”",
                    cell(line, "rule"),
                    library.entries()[known].name
                ));
            }
        }
    }
    said
}

/// Prints some columns of some lines, under a title; nothing if there are no lines.
fn show(title: &str, lines: &[&Vec<String>], columns: &[&str]) {
    if lines.is_empty() {
        return;
    }
    let cell = |line: &Vec<String>, name: &str| line[column(name)].clone();
    let width = |name: &str| lines.iter().map(|line| cell(line, name).len()).max().unwrap_or(0).max(name.len());
    let print = |cells: Vec<String>| {
        let padded: Vec<String> =
            cells.iter().zip(columns).map(|(cell, name)| format!("{cell:<width$}", width = width(name))).collect();
        println!("{}", padded.join("  ").trim_end());
    };
    println!("\n{title}");
    print(columns.iter().map(|name| name.to_string()).collect());
    for line in lines {
        print(columns.iter().map(|name| cell(line, name)).collect());
    }
}

/// The rules of the family to measure now, those of the table left out, with what to say of
/// them and how many there were to measure; or why none are measured. A family of more than
/// `most` rules is not gone through.
fn of_family(args: &Args, done: &HashSet<String>, most: u64) -> Result<(String, Vec<BlockRule>, usize), String> {
    let family = Family::new(args.family.iter().flat_map(|family| family.constraints().iter().copied()));
    let named = match family.constraints() {
        [] => "every rule there is".to_string(),
        constraints => constraints.iter().map(|constraint| constraint.to_string()).collect::<Vec<_>>().join("+"),
    };
    // Going through a large family takes a while: it says how far it has got.
    let progress = Progress::default();
    let listed = std::thread::scope(|scope| {
        let going = scope.spawn(|| family.canonical_rules(args.threads, most, &progress));
        let started = Instant::now();
        let mut said = Duration::ZERO;
        while !going.is_finished() {
            std::thread::sleep(Duration::from_millis(50));
            if started.elapsed() >= said + Duration::from_secs(5) {
                said = started.elapsed();
                eprintln!("{named}: {} rules gone through", progress.rules());
            }
        }
        going.join().expect("going through the family did not fail")
    });
    let Some((rules, worlds)) = listed else {
        let too_many = format!("{named}: more than {most} rules, too many to go through");
        let Some(limit) = args.limit.filter(|_| args.sample) else {
            return Err(format!("{too_many}. --sample with --limit N measures N of them, drawn at random."));
        };
        let (drawn, draws) = draw(&family, limit, args.seed, done);
        let said =
            format!("{too_many}; {} canonical rules drawn with seed {} in {draws} draws", drawn.len(), args.seed);
        let left = drawn.len();
        return Ok((said, drawn, left));
    };
    let said = format!("{named}: {rules} rules, {} canonical", worlds.len());
    let mut todo: Vec<BlockRule> = worlds.into_iter().filter(|rule| !done.contains(&rule.to_string())).collect();
    let left = todo.len();
    if let Some(limit) = args.limit.filter(|limit| left > *limit && !args.sample) {
        return Err(format!(
            "{said}, {left} of them still to measure: more than --limit {limit}. Leave out --limit to measure them \
             all, or add --sample to measure {limit} of them picked at random."
        ));
    }
    shuffle(&mut todo);
    todo.truncate(args.limit.unwrap_or(left));
    Ok((said, todo, left))
}

/// So many canonical rules of the family drawn at random, different from each other and from
/// those already measured, and how many draws that took: fewer of them if a thousand draws in
/// a row find nothing new.
fn draw(family: &Family, wanted: usize, seed: u64, done: &HashSet<String>) -> (Vec<BlockRule>, usize) {
    let mut rng = Rng::new(seed);
    let (mut drawn, mut seen, mut draws, mut in_vain) = (Vec::new(), HashSet::new(), 0, 0);
    while drawn.len() < wanted && in_vain < 1000 {
        draws += 1;
        match family.draw(&mut rng).map(|rule| rule.canonical()) {
            Some(rule) if !done.contains(&rule.to_string()) && seen.insert(rule.clone()) => {
                drawn.push(rule);
                in_vain = 0;
            }
            _ => in_vain += 1,
        }
    }
    (drawn, draws)
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

    #[test]
    fn a_family_is_measured_whole_unless_a_sample_is_asked_for() {
        let args =
            |words: &str| Args::try_parse_from(["cas-search"].into_iter().chain(words.split_whitespace())).unwrap();
        let none = HashSet::new();
        // All of it, in canonical form.
        let (said, todo, left) = of_family(&args("--family quarter-turn"), &none, COUNTABLE).unwrap();
        assert_eq!((said.as_str(), todo.len(), left), ("quarter-turn: 1536 rules, 584 canonical", 584, 584));
        assert!(todo.iter().all(|rule| rule.canonical() == *rule));
        assert_eq!(of_family(&args("--family quarter-turn --limit 584"), &none, COUNTABLE).unwrap().1.len(), 584);
        // More than the limit is not measured at all, unless a sample is asked for.
        let refused = of_family(&args("--family quarter-turn --limit 100"), &none, COUNTABLE).unwrap_err();
        assert!(refused.contains("584 of them still to measure") && refused.contains("--sample"), "{refused}");
        let (_, sample, left) =
            of_family(&args("--family quarter-turn --limit 100 --sample"), &none, COUNTABLE).unwrap();
        assert_eq!((sample.len(), left), (100, 584));
        // What the table has already is left out, and counts no longer.
        let done: HashSet<String> = sample.iter().map(|rule| rule.to_string()).collect();
        let (_, rest, left) = of_family(&args("--family quarter-turn --limit 484"), &done, COUNTABLE).unwrap();
        assert_eq!((rest.len(), left), (484, 484));
        assert!(rest.iter().all(|rule| !done.contains(&rule.to_string())));
        // A family too big to go through is only drawn from: as many canonical rules as asked
        // for, all different.
        let refused = of_family(&args("--family half-turn"), &none, 1000).unwrap_err();
        assert!(refused.starts_with("half-turn: more than 1000 rules, too many to go through"), "{refused}");
        let (said, drawn, _) =
            of_family(&args("--family half-turn --limit 50 --sample --seed 3"), &none, 1000).unwrap();
        assert!(said.contains("50 canonical rules drawn with seed 3"), "{said}");
        let unique: HashSet<&BlockRule> = drawn.iter().collect();
        assert_eq!((drawn.len(), unique.len()), (50, 50));
        assert!(drawn.iter().all(|rule| rule.canonical() == *rule));
        // A sample says how many.
        assert!(Args::try_parse_from(["cas-search", "--sample"]).is_err());
    }

    #[test]
    fn the_best_finds_are_kept_once() {
        let line = |rule: &str, character: &str, spaceships: &str| {
            let mut line = vec![String::new(); COLUMNS.len()];
            line[column("rule")] = rule.to_string();
            line[column("character")] = character.to_string();
            line[column("spaceships")] = spaceships.to_string();
            line[column("periods")] = "3".to_string();
            line[column("longest")] = "48".to_string();
            line
        };
        // Best first: a world with ships, Critters (which the library has), what explodes, and
        // a world with one kind of ship.
        let lines = [
            line("0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15", "spaceships", "5"),
            line("15,14,13,3,11,5,6,1,7,9,10,2,12,4,8,0", "spaceships", "4"),
            line("0,3,13,1,11,10,6,7,12,9,5,4,8,2,14,15", "explosive", "0"),
            line("0,4,8,3,1,5,6,7,2,9,10,11,12,13,14,15", "spaceships", "1"),
        ];
        let mut library = Library::new();
        let said = keep(&mut library, &lines, 5, "half-turn");
        // The names are numbered as the rules are kept, whatever their places.
        assert_eq!(
            said,
            [
                "kept 0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15 as “half-turn 1”",
                "15,14,13,3,11,5,6,1,7,9,10,2,12,4,8,0 is in the library already, as “Critters”",
                "kept 0,4,8,3,1,5,6,7,2,9,10,11,12,13,14,15 as “half-turn 2”",
            ]
        );
        let kept: Vec<_> = library.entries().iter().filter(|entry| entry.kept()).collect();
        assert_eq!(kept[0].name, "half-turn 1");
        assert_eq!(kept[0].tags, ["search"]);
        assert_eq!(kept[0].note, "spaceships: 5 kinds of spaceship, 3 periods, the longest 48");
        assert_eq!(kept[1].note, "spaceships: 1 kind of spaceship, 3 periods, the longest 48");
        // A second run keeps nothing twice.
        assert_eq!(keep(&mut library, &lines, 1, "half-turn").len(), 1);
        assert_eq!(library.entries().iter().filter(|entry| entry.kept()).count(), 2);
    }

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
