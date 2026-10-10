//! cas-search — looks for interesting rules: puts every rule of a family through the trials of
//! `cas_core::search` and writes one line per rule.

use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashSet},
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use cas_core::{
    collection::{self, Collection, Kept, Sort},
    families::{self, COUNTABLE, Family, Progress},
    library::{Library, usual_file},
    pattern::{Heading, Motion},
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
    /// Measure these rules instead of a family: presets, ESPCA numbers, tables, or tables in
    /// hex.
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
    /// Keep the patterns found: the spaceships, the oscillators and the still lifes of every
    /// rule measured in this run that is a find, or that was named with --rule, each in the
    /// file of its rule in the folder of the app's kept patterns, `patterns` next to
    /// `rules.tsv`, or in DIR. A rule that an --out table has already is not measured again,
    /// so nothing of it is kept either. Without this nothing is kept.
    #[arg(long, value_name = "DIR", num_args = 0..=1)]
    patterns: Option<Option<PathBuf>>,
    /// With --patterns, keep only these sorts of pattern: spaceships, oscillators and
    /// still-lifes, with commas between; all three unless said. Under some rules there are
    /// thousands of oscillators and little in them.
    #[arg(long, value_delimiter = ',', value_name = "SORT")]
    sorts: Vec<Sort>,
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

const COLUMNS: [&str; 25] = [
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
    "speeds",
    "fastest",
    "slowest",
    "headings",
    "periods",
    "longest",
    "oscillators",
    "still-lifes",
    "damage",
    "blob",
    "remaining",
    "caught",
    "others",
];

fn column(name: &str) -> usize {
    COLUMNS.iter().position(|&column| column == name).expect("a column of the table")
}

/// What the spaceships of a report come to: their speeds, each once; the fastest and the
/// slowest; and whether they fly straight, along a diagonal or neither.
#[derive(Default)]
struct Ships {
    speeds: BTreeSet<(u32, u32)>,
    fastest: Option<(u32, u32)>,
    slowest: Option<(u32, u32)>,
    headings: Vec<&'static str>,
}

fn ships_of(report: &Report) -> Ships {
    let mut ships = Ships::default();
    // Speeds compared as fractions, without dividing.
    let faster = |a: (u32, u32), b: (u32, u32)| (a.0 as u64 * b.1 as u64) > (b.0 as u64 * a.1 as u64);
    for found in report.found.iter().filter(|found| found.sort == Sort::Spaceship) {
        let motion = Motion { period: found.period, displacement: found.moves, canonical: Vec::new() };
        let speed = motion.speed();
        ships.speeds.insert(speed);
        ships.fastest = Some(ships.fastest.filter(|&fastest| !faster(speed, fastest)).unwrap_or(speed));
        ships.slowest = Some(ships.slowest.filter(|&slowest| !faster(slowest, speed)).unwrap_or(speed));
        let heading = match motion.heading() {
            Heading::Orthogonal => "orthogonal",
            Heading::Diagonal => "diagonal",
            _ => "oblique",
        };
        if !ships.headings.contains(&heading) {
            ships.headings.push(heading);
        }
    }
    // In one order, whatever came first: straight, along a diagonal, neither.
    ships.headings.sort_by_key(|heading| ["orthogonal", "diagonal", "oblique"].iter().position(|h| h == heading));
    ships
}

/// A speed as the app writes one: so many cells in so many generations, as a fraction of the
/// speed of light.
fn speed_name((cells, period): (u32, u32)) -> String {
    match (cells, period) {
        (1, 1) => "c".to_string(),
        (1, period) => format!("c/{period}"),
        (cells, period) => format!("{cells}c/{period}"),
    }
}

/// A line of the table. Shares of the seeds are in percent, as is the damage; what was not
/// tried is left empty.
fn line(rule: &BlockRule, report: &Report) -> Vec<String> {
    let ships = ships_of(report);
    let of_sort = |sort: Sort| report.found.iter().filter(|found| found.sort == sort).count();
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
        ships.speeds.len().to_string(),
        ships.fastest.map(speed_name).unwrap_or_default(),
        ships.slowest.map(speed_name).unwrap_or_default(),
        ships.headings.join("+"),
        report.periods.to_string(),
        report.longest_period.to_string(),
        of_sort(Sort::Oscillator).to_string(),
        of_sort(Sort::StillLife).to_string(),
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

/// The worth of a rule, as the lines of the table are sorted: the worlds with things that
/// travel and things that stay come first, by how many speeds their spaceships have, then by
/// their kinds of spaceship, then by the kinds that stay. Then the rules whose seeds grow
/// along lines and send out spaceships, by the same. What explodes, ignites or stands still
/// is no find.
fn merit(line: &[String]) -> (u8, u64, u64, u64) {
    let number = |name: &str| line[column(name)].parse::<u64>().unwrap_or(0);
    let (spaceships, speeds) = (number("spaceships"), number("speeds"));
    let stays = number("oscillators") + number("still-lifes");
    match line[column("character")].as_str() {
        "spaceships" => (2, speeds, spaceships, stays),
        "linear" if spaceships > 0 => (1, speeds, spaceships, stays),
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
    // The patterns found go to the folder, if one was asked for: of the finds, and of the
    // rules that were named.
    let folder = args.patterns.as_ref().map(|folder| folder.clone().unwrap_or_else(collection::usual_folder));
    let named = !args.rules.is_empty();
    // The table, the lines of this run, when progress was last reported, and how many
    // patterns were kept, of how many rules.
    let progress = Mutex::new((file, Vec::new(), started, (0usize, 0usize)));
    search::survey(&todo, &effort, |rule, report| {
        let line = line(rule, &report);
        let mut progress = progress.lock().unwrap();
        let (file, measured, reported, kept) = &mut *progress;
        // The patterns before the line: a rule whose patterns could not be kept is not in
        // the table as done, and is measured again next time.
        if let Some(folder) = &folder
            && (named || merit(&line).0 > 0)
        {
            let new = keep_patterns(folder, rule, &report, &args.sorts).unwrap_or_else(|error| fail(&error));
            kept.0 += new;
            kept.1 += usize::from(new > 0);
        }
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
    let (_, measured, _, kept) = progress.into_inner().unwrap();
    eprintln!("{} rules measured in {}", measured.len(), clock(started.elapsed().as_secs_f64()));
    if let Some(folder) = &folder {
        eprintln!("{} patterns kept for {} rules in {}", kept.0, kept.1, folder.display());
    }
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
        "The best worlds, with things that travel and things that stay, by their speeds:",
        &best(2),
        &[
            "rule",
            "espca",
            "speeds",
            "fastest",
            "slowest",
            "headings",
            "spaceships",
            "oscillators",
            "still-lifes",
            "blob",
            "cells",
        ],
    );
    show(
        "The best of the rules whose seeds grow along lines, by the speeds of the spaceships they send out:",
        &best(1),
        &["rule", "espca", "growing", "growth", "speeds", "spaceships", "blob"],
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

/// Keeps what a rule's report found in the rule's file of patterns, in the folder, among
/// what was kept there already: how many were new. Only the `sorts` asked for, or every sort
/// given none. A pattern that cannot be kept ends the search, as a line that cannot be
/// written does.
fn keep_patterns(folder: &Path, rule: &BlockRule, report: &Report, sorts: &[Sort]) -> Result<usize, String> {
    let file = Collection::file(folder, rule);
    let mut kept = Collection::read_file(&file, rule).map_err(|error| format!("{}: {error}", file.display()))?;
    let mut new = 0;
    for found in report.found.iter().filter(|found| sorts.is_empty() || sorts.contains(&found.sort)) {
        let mut pattern = Kept::new(rule, found.sort, &found.cells, found.period, found.moves);
        pattern.note = "search".to_string();
        new += usize::from(kept.keep(pattern).is_ok());
    }
    if new > 0 {
        kept.write(folder).map_err(|error| format!("cannot write {}: {error}", file.display()))?;
    }
    Ok(new)
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
    // The whole family in one order, whatever the table holds already: runs cut differently
    // measure the same rules.
    let mut worlds = worlds;
    shuffle(&mut worlds);
    let mut todo: Vec<BlockRule> = worlds.into_iter().filter(|rule| !done.contains(&rule.to_string())).collect();
    let left = todo.len();
    if let Some(limit) = args.limit.filter(|limit| left > *limit && !args.sample) {
        return Err(format!(
            "{said}, {left} of them still to measure: more than --limit {limit}. Leave out --limit to measure them \
             all, or add --sample to measure {limit} of them picked at random."
        ));
    }
    todo.truncate(args.limit.unwrap_or(left));
    Ok((said, todo, left))
}

/// So many canonical rules of the family drawn at random, different from each other and from
/// those already measured, and how many draws that took: fewer of them if so many draws in a
/// row find nothing new, a thousand and one more for every rule the table holds, since a
/// search taken up again draws the rules it measured before first.
fn draw(family: &Family, wanted: usize, seed: u64, done: &HashSet<String>) -> (Vec<BlockRule>, usize) {
    let mut rng = Rng::new(seed);
    let patience = 1000 + done.len();
    let (mut drawn, mut seen, mut draws, mut in_vain) = (Vec::new(), HashSet::new(), 0, 0);
    while drawn.len() < wanted && in_vain < patience {
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

/// Always the same shuffle, of the whole family: what is left of it after some runs does not
/// depend on how the runs were cut.
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
        // Runs cut differently measure the same rules: two samples of a hundred, the second
        // with the first in the table, are one sample of two hundred.
        let (_, second, _) = of_family(&args("--family quarter-turn --limit 100 --sample"), &done, COUNTABLE).unwrap();
        let (_, two_hundred, _) =
            of_family(&args("--family quarter-turn --limit 200 --sample"), &none, COUNTABLE).unwrap();
        let sorted = |rules: &[BlockRule]| {
            let mut rules: Vec<String> = rules.iter().map(|rule| rule.to_string()).collect();
            rules.sort();
            rules
        };
        assert_eq!(sorted(&[sample.clone(), second].concat()), sorted(&two_hundred));
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
        // A sample taken up again draws the rules it measured before first, however many,
        // and goes on to new ones.
        let (_, before, _) =
            of_family(&args("--family half-turn --limit 1200 --sample --seed 3"), &none, 1000).unwrap();
        let measured: HashSet<String> = before.iter().map(|rule| rule.to_string()).collect();
        let (_, more, _) =
            of_family(&args("--family half-turn --limit 50 --sample --seed 3"), &measured, 1000).unwrap();
        assert_eq!(more.len(), 50);
        assert!(more.iter().all(|rule| !measured.contains(&rule.to_string())));
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
    fn a_report_is_summed_up_and_its_patterns_kept() {
        use cas_core::{pattern::from_rle, search::Found};
        let found = |sort, rle: &str, period, moves| Found { sort, cells: from_rle(rle).unwrap(), period, moves };
        let rule: BlockRule = "single-rotation".parse().unwrap();
        let report = Report {
            found: vec![
                found(Sort::Spaceship, "b2o2$b2o", 12, (2, 0)),
                found(Sort::Spaceship, "3o$o$bo", 15, (1, 1)),
                found(Sort::Spaceship, "2o$o", 8, (2, 2)),
                found(Sort::Spaceship, "o$2o", 24, (4, 0)),
                found(Sort::Spaceship, "3o", 16, (2, 0)),
                found(Sort::Oscillator, "o", 4, (0, 0)),
                found(Sort::StillLife, "$2o$2o", 1, (0, 0)),
            ],
            ..search::measure(&rule, &Effort { seeds: 0, generations: 10, blob: 0 })
        };
        // The speeds each once (c/6 and 2c/12 are one), the fastest and the slowest as
        // fractions, and the headings in one order whatever came first.
        let ships = ships_of(&report);
        assert_eq!(ships.speeds.len(), 4);
        assert_eq!((ships.fastest, ships.slowest), (Some((1, 4)), Some((1, 15))));
        assert_eq!(ships.headings, ["orthogonal", "diagonal"]);
        assert_eq!([speed_name((1, 1)), speed_name((1, 6)), speed_name((2, 5))], ["c", "c/6", "2c/5"]);
        let cells = line(&rule, &report);
        assert_eq!(cells.len(), COLUMNS.len());
        let cell = |name: &str| cells[column(name)].as_str();
        assert_eq!((cell("speeds"), cell("fastest"), cell("slowest")), ("4", "c/4", "c/15"));
        assert_eq!((cell("headings"), cell("oscillators"), cell("still-lifes")), ("orthogonal+diagonal", "1", "1"));
        // Kept in the rule's file, with the note that says where they came from; kept again,
        // nothing is new, and what the file had stays.
        let every = folder("patterns");
        assert_eq!(keep_patterns(&every, &rule, &report, &[]).unwrap(), 7);
        assert_eq!(keep_patterns(&every, &rule, &report, &[]).unwrap(), 0);
        let kept = Collection::read(&every).unwrap();
        assert_eq!(kept.all().len(), 7);
        assert!(kept.all().iter().all(|kept| kept.note == "search" && kept.rule == rule));
        let _ = std::fs::remove_dir_all(&every);
        // Only the sorts asked for.
        let some = folder("some-patterns");
        assert_eq!(keep_patterns(&some, &rule, &report, &[Sort::Spaceship, Sort::StillLife]).unwrap(), 6);
        let kept = Collection::read(&some).unwrap();
        assert!(kept.all().iter().all(|kept| kept.sort != Sort::Oscillator));
        let _ = std::fs::remove_dir_all(&some);
    }

    #[test]
    fn worlds_with_things_that_travel_and_things_that_stay_come_first() {
        let line = |character: &str, speeds: u64, spaceships: u64, oscillators: u64, stills: u64| {
            let mut cells = vec![String::new(); COLUMNS.len()];
            cells[column("character")] = character.to_string();
            cells[column("speeds")] = speeds.to_string();
            cells[column("spaceships")] = spaceships.to_string();
            cells[column("oscillators")] = oscillators.to_string();
            cells[column("still-lifes")] = stills.to_string();
            cells
        };
        // By the speeds of the spaceships, then their kinds, then the kinds that stay; then
        // guns by the same; the rest is no find.
        assert!(merit(&line("spaceships", 8, 10, 0, 0)) > merit(&line("spaceships", 7, 30, 50, 50)));
        assert!(merit(&line("spaceships", 3, 5, 0, 0)) > merit(&line("spaceships", 3, 4, 9, 9)));
        assert!(merit(&line("spaceships", 3, 4, 5, 4)) > merit(&line("spaceships", 3, 4, 8, 0)));
        assert!(merit(&line("spaceships", 1, 1, 0, 0)) > merit(&line("linear", 9, 9, 9, 9)));
        assert!(merit(&line("linear", 2, 2, 0, 0)) > merit(&line("linear", 1, 5, 5, 5)));
        assert_eq!(merit(&line("linear", 0, 0, 3, 3)), merit(&line("explosive", 5, 5, 5, 5)));
    }
}
