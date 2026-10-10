//! The collection: the patterns that were kept, rule by rule.
//!
//! A pattern that comes back to its shape is worth keeping: a spaceship, an oscillator, a
//! still life. The kept ones lie in a folder next to the library's file, `patterns`, a plain
//! text file to a rule, named by the rule's table in hex, and a pattern to a line. The files
//! are meant to be read, edited by hand and kept under version control, and the app, the
//! search and whatever else looks at rules may all add to them:
//!
//! ```text
//! patterns/0283156749abcdef.tsv
//!
//! sort        pattern   period  moves  name  note
//! spaceship   b2o2$b2o  12      2,0          the lightest
//! still life  2o$2o     1
//! ```
//!
//! The columns are separated by tabs, one between any two (they are drawn apart here), and
//! go by the names in the line that begins with `sort`, as the library's do. The rule is the
//! file's name ([`BlockRule::hex`]), and its table in full is in the comment the file begins
//! with. A pattern is written as the app writes one: run-length encoded, from a corner of the
//! blocks the next step rewrites, at the start of the vacuum's cycle. It is the form its kind
//! is filed under ([`Motion::canonical`](crate::pattern::Motion)), so that a kind is kept
//! once whichever way it was found lying.

use std::{
    collections::{HashMap, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    library::{self, Fields},
    pattern::{Cell, from_rle, settled, to_rle},
    rules::BlockRule,
};

/// What a rule's file begins with: whose it is, what it holds, and its columns by name.
fn heading(rule: &BlockRule) -> String {
    let named = rule.preset().map(|preset| format!(" ({})", preset.name)).unwrap_or_default();
    format!(
        "# The patterns kept for cas under the rule {rule}{named}:\n\
         # a pattern to a line, with tabs in between: what it is (a spaceship, an oscillator or a\n\
         # still life), the pattern (run-length encoded, from a corner of the blocks the next step\n\
         # rewrites, at the start of the vacuum's cycle), its period, how far it moves in a period,\n\
         # a name, and a note. The file is named by the rule's table in hex.\n\
         sort\tpattern\tperiod\tmoves\tname\tnote\n"
    )
}

/// The columns of a file, in the order they are written.
const COLUMNS: [&str; 6] = ["sort", "pattern", "period", "moves", "name", "note"];

/// The sorts of pattern that are kept: what comes back to its shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sort {
    Spaceship,
    Oscillator,
    StillLife,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Spaceship, Sort::Oscillator, Sort::StillLife];

    /// The sort of a pattern that is back after a period, having moved so far: `still` if it
    /// did not change on the way either.
    pub fn of(moves: (i32, i32), still: bool) -> Self {
        match (moves, still) {
            ((0, 0), true) => Sort::StillLife,
            ((0, 0), false) => Sort::Oscillator,
            _ => Sort::Spaceship,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Sort::Spaceship => "spaceship",
            Sort::Oscillator => "oscillator",
            Sort::StillLife => "still life",
        }
    }

    /// Several of them.
    pub fn names(self) -> &'static str {
        match self {
            Sort::Spaceship => "spaceships",
            Sort::Oscillator => "oscillators",
            Sort::StillLife => "still lifes",
        }
    }
}

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Sort {
    type Err = String;

    /// One or several: `spaceship`, `still lifes`, `still-lifes`.
    fn from_str(text: &str) -> Result<Self, String> {
        let wanted: String = text.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
        let named = |sort: &Sort| [sort.name(), sort.names()].iter().any(|name| name.replace(' ', "") == wanted);
        Sort::ALL.into_iter().find(named).ok_or_else(|| format!("{text:?} is no sort of pattern"))
    }
}

/// A pattern that was kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub rule: BlockRule,
    pub sort: Sort,
    /// The form the kind is filed under, relative to a corner of the blocks the next step
    /// rewrites, at the start of the vacuum's cycle.
    pub cells: Vec<Cell>,
    pub period: u32,
    /// How far that form moves in a period.
    pub moves: (i32, i32),
    pub name: String,
    pub note: String,
}

impl Kept {
    /// A pattern to keep, without a name or a note yet.
    pub fn new(rule: &BlockRule, sort: Sort, cells: &[Cell], period: u32, moves: (i32, i32)) -> Self {
        Self {
            rule: rule.clone(),
            sort,
            cells: settled(cells),
            period,
            moves,
            name: String::new(),
            note: String::new(),
        }
    }
}

/// The patterns that were kept, rule by rule, each rule's in the order they were kept.
#[derive(Clone, Debug, Default)]
pub struct Collection {
    rules: HashMap<BlockRule, Shelf>,
    /// The rules whose patterns changed since their files were last written.
    changed: HashSet<BlockRule>,
}

/// A rule's patterns, in the order they were kept; and the cells of each, so that one that is
/// kept already is known at once, however many there are.
#[derive(Clone, Debug, Default)]
struct Shelf {
    kept: Vec<Kept>,
    cells: HashSet<Vec<Cell>>,
}

impl PartialEq for Collection {
    fn eq(&self, other: &Self) -> bool {
        self.rules.len() == other.rules.len()
            && self
                .rules
                .iter()
                .all(|(rule, shelf)| other.rules.get(rule).is_some_and(|theirs| theirs.kept == shelf.kept))
    }
}

impl Collection {
    /// The file of a rule's patterns in a folder: named by the rule's table in hex.
    pub fn file(folder: &Path, rule: &BlockRule) -> PathBuf {
        folder.join(format!("{}.tsv", rule.hex()))
    }

    /// The patterns of a rule that its file describes. A line that cannot be read is an
    /// error, with its number: a file read in part would lose the rest when it is written
    /// again.
    pub fn parse(text: &str, rule: &BlockRule) -> Result<Self, String> {
        let mut collection = Self::default();
        let mut line = Fields::of(&COLUMNS);
        for (number, text) in text.lines().enumerate() {
            if text.trim().is_empty() || text.starts_with('#') || line.read(text) {
                continue;
            }
            let (period, moves) = (line.get("period"), line.get("moves"));
            let wrong = |error: String| format!("line {}: {error}", number + 1);
            let sort: Sort = line.get("sort").parse().map_err(wrong)?;
            let cells = from_rle(line.get("pattern")).map_err(wrong)?;
            if cells.is_empty() {
                return Err(wrong("a pattern has cells".to_string()));
            }
            let period = period.parse().map_err(|_| wrong(format!("{period:?} is no period")))?;
            let moves = match moves.split_once(',') {
                None if moves.is_empty() => (0, 0),
                Some((dx, dy)) => match (dx.trim().parse(), dy.trim().parse()) {
                    (Ok(dx), Ok(dy)) => (dx, dy),
                    _ => return Err(wrong(format!("{moves:?} is no way to move"))),
                },
                None => return Err(wrong(format!("{moves:?} is no way to move"))),
            };
            let (name, note) = (line.get("name").to_string(), line.get("note").to_string());
            let kept = Kept { name, note, ..Kept::new(rule, sort, &cells, period, moves) };
            // The same pattern twice is the same pattern.
            let _ = collection.keep(kept);
        }
        // What was read is as the file has it.
        collection.changed.clear();
        Ok(collection)
    }

    /// The patterns of a rule, from its file. Where there is no file, none were kept.
    pub fn read_file(path: &Path, rule: &BlockRule) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text, rule),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// The collection of a folder: the patterns of every file in it that is a rule's, named
    /// by the rule's table in hex, in lower case. Where there is no folder yet, nothing was
    /// kept. A file that cannot be read is an error, with its name, and so is one named like
    /// a rule's that is no rule's: a table mistyped, or in capitals, would be a rule twice
    /// over. Other files are left alone.
    pub fn read(folder: &Path) -> Result<Self, String> {
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.to_string()),
        };
        let mut collection = Self::default();
        for entry in entries.flatten() {
            let path = entry.path();
            let (extension, stem) =
                (path.extension().and_then(|e| e.to_str()), path.file_stem().and_then(|s| s.to_str()));
            let (Some("tsv"), Some(stem)) = (extension, stem) else {
                continue;
            };
            if stem.len() != 16 || !stem.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let Some(rule) = BlockRule::from_hex(stem).filter(|rule| rule.hex() == stem) else {
                return Err(format!(
                    "{}: not a rule's file: a file is named by its rule's table in hex, in lower case",
                    path.display()
                ));
            };
            let file = Self::read_file(&path, &rule).map_err(|error| format!("{}: {error}", path.display()))?;
            collection.take(&rule, file);
        }
        Ok(collection)
    }

    /// Writes the files of the rules whose patterns changed, each whole or not at all, as the
    /// library writes its own file; the file of a rule with no patterns left is removed.
    pub fn write(&mut self, folder: &Path) -> io::Result<()> {
        fs::create_dir_all(folder)?;
        let changed: Vec<BlockRule> = self.changed.iter().cloned().collect();
        for rule in changed {
            let path = Self::file(folder, &rule);
            match self.text_of(&rule) {
                Some(text) => library::write_whole(&path, &text)?,
                None => match fs::remove_file(&path) {
                    Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                    _ => {}
                },
            }
            self.changed.remove(&rule);
        }
        Ok(())
    }

    /// A rule's file: its patterns in the order they were kept. None for a rule with none.
    pub fn text_of(&self, rule: &BlockRule) -> Option<String> {
        let shelf = self.rules.get(rule)?;
        let mut text = heading(rule);
        let line = |text: &str| text.split(['\t', '\n', '\r']).collect::<Vec<_>>().join(" ").trim().to_string();
        for kept in &shelf.kept {
            let moves = match kept.moves {
                (0, 0) => String::new(),
                (dx, dy) => format!("{dx},{dy}"),
            };
            let (name, note) = (line(&kept.name), line(&kept.note));
            text.push_str(&format!(
                "{}\t{}\t{}\t{moves}\t{name}\t{note}\n",
                kept.sort,
                to_rle(&kept.cells),
                kept.period
            ));
        }
        Some(text)
    }

    /// Every pattern kept: rule by rule, in the order of their tables, and each rule's in the
    /// order they were kept.
    pub fn all(&self) -> Vec<&Kept> {
        let mut rules: Vec<&BlockRule> = self.rules.keys().collect();
        rules.sort_unstable_by(|a, b| a.table().cmp(b.table()));
        rules.into_iter().flat_map(|rule| self.under(rule)).collect()
    }

    /// The patterns kept under a rule, in the order they were kept.
    pub fn under(&self, rule: &BlockRule) -> &[Kept] {
        self.rules.get(rule).map_or(&[], |shelf| &shelf.kept)
    }

    /// The patterns of one sort kept under a rule, in the order they were kept.
    pub fn of<'a>(&'a self, rule: &BlockRule, sort: Sort) -> impl Iterator<Item = &'a Kept> + use<'a> {
        self.under(rule).iter().filter(move |kept| kept.sort == sort)
    }

    /// Whether a pattern of a rule is kept. The cells are those of the form its kind is filed
    /// under.
    pub fn is_kept(&self, rule: &BlockRule, cells: &[Cell]) -> bool {
        self.rules.get(rule).is_some_and(|shelf| shelf.cells.contains(&settled(cells)))
    }

    /// Where among the rule's patterns a pattern is, if it was kept.
    pub fn find(&self, rule: &BlockRule, cells: &[Cell]) -> Option<usize> {
        let shelf = self.rules.get(rule)?;
        let cells = settled(cells);
        if !shelf.cells.contains(&cells) {
            return None;
        }
        shelf.kept.iter().position(|kept| kept.cells == cells)
    }

    /// Keeps a pattern. One that is kept already is not kept twice: then the place it has
    /// comes back as the error.
    pub fn keep(&mut self, kept: Kept) -> Result<usize, usize> {
        let cells = settled(&kept.cells);
        if let Some(known) = self.find(&kept.rule, &cells) {
            return Err(known);
        }
        self.changed.insert(kept.rule.clone());
        let shelf = self.rules.entry(kept.rule.clone()).or_default();
        shelf.cells.insert(cells.clone());
        shelf.kept.push(Kept { cells, ..kept });
        Ok(shelf.kept.len() - 1)
    }

    /// Lets go of a pattern of a rule. True if it was kept.
    pub fn forget(&mut self, rule: &BlockRule, cells: &[Cell]) -> bool {
        let Some(index) = self.find(rule, cells) else {
            return false;
        };
        let shelf = self.rules.get_mut(rule).expect("the pattern was found under the rule");
        let gone = shelf.kept.remove(index);
        shelf.cells.remove(&gone.cells);
        if shelf.kept.is_empty() {
            self.rules.remove(rule);
        }
        self.changed.insert(rule.clone());
        true
    }

    /// Takes a rule's patterns from another collection, in place of what was held of the
    /// rule: what its file says, read again. Nothing of the rule is left to write.
    pub fn take(&mut self, rule: &BlockRule, mut from: Collection) {
        match from.rules.remove(rule) {
            Some(shelf) => self.rules.insert(rule.clone(), shelf),
            None => self.rules.remove(rule),
        };
        self.changed.remove(rule);
    }
}

/// The folder of the collection where no other is named: `patterns`, next to the library's
/// own file.
pub fn usual_folder() -> PathBuf {
    library::usual_file().with_file_name("patterns")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(text: &str) -> BlockRule {
        text.parse().unwrap()
    }

    fn cells(rle: &str) -> Vec<Cell> {
        from_rle(rle).unwrap()
    }

    #[test]
    fn a_pattern_is_kept_once_under_its_rule() {
        let (rotation, critters) = (rule("single-rotation"), rule("critters"));
        let mut collection = Collection::default();
        let ship = Kept::new(&rotation, Sort::Spaceship, &cells("b2o2$b2o"), 12, (2, 0));
        assert_eq!(collection.keep(ship.clone()), Ok(0));
        assert_eq!(collection.keep(ship.clone()), Err(0));
        // The same cells two blocks over are the same pattern; a cell over, or under another
        // rule, they are another.
        let moved: Vec<Cell> = ship.cells.iter().map(|&(x, y)| (x + 4, y + 2)).collect();
        assert_eq!(collection.find(&rotation, &moved), Some(0));
        let shifted: Vec<Cell> = ship.cells.iter().map(|&(x, y)| (x + 1, y)).collect();
        assert_eq!((collection.find(&rotation, &shifted), collection.find(&critters, &ship.cells)), (None, None));
        // Each rule's are in a place of their own.
        assert_eq!(collection.keep(Kept::new(&critters, Sort::Oscillator, &ship.cells, 4, (0, 0))), Ok(0));
        assert_eq!(collection.keep(Kept::new(&rotation, Sort::StillLife, &cells("2o$2o"), 1, (0, 0))), Ok(1));
        // Each rule has its own, sort by sort.
        let of = |collection: &Collection, rule: &BlockRule, sort| collection.of(rule, sort).count();
        assert_eq!(of(&collection, &rotation, Sort::Spaceship), 1);
        assert_eq!((of(&collection, &rotation, Sort::StillLife), of(&collection, &rotation, Sort::Oscillator)), (1, 0));
        assert_eq!((of(&collection, &critters, Sort::Oscillator), of(&collection, &critters, Sort::Spaceship)), (1, 0));
        // What was kept can be let go of.
        assert!(collection.is_kept(&rotation, &moved) && !collection.is_kept(&rotation, &shifted));
        assert!(collection.forget(&rotation, &moved) && !collection.forget(&rotation, &moved));
        assert_eq!((collection.all().len(), of(&collection, &rotation, Sort::Spaceship)), (2, 0));
        assert!(!collection.is_kept(&rotation, &moved));
        // A rule's patterns taken from another collection are in place of what was held.
        let mut other = Collection::default();
        other.keep(Kept::new(&rotation, Sort::Oscillator, &cells("o"), 4, (0, 0))).unwrap();
        collection.take(&rotation, other);
        assert_eq!(collection.under(&rotation).len(), 1);
        assert_eq!(of(&collection, &critters, Sort::Oscillator), 1);
        collection.take(&critters, Collection::default());
        assert_eq!((collection.all().len(), collection.under(&critters).len()), (1, 0));
    }

    #[test]
    fn the_file_says_what_was_kept() {
        let slow = rule("15,7,6,10,13,12,2,8,14,11,5,4,3,9,1,0");
        let mut collection = Collection::default();
        let mut ship = Kept::new(&slow, Sort::Spaceship, &cells("bobo$obo"), 7_328_092, (0, -8));
        ship.name = "The\tslowest".to_string();
        ship.note = "eight cells up\nin a period".to_string();
        collection.keep(ship).unwrap();
        collection.keep(Kept::new(&rule("single-rotation"), Sort::StillLife, &cells("2o$2o"), 1, (0, 0))).unwrap();
        // A file to a rule, which says whose it is.
        let file = collection.text_of(&slow).unwrap();
        assert!(
            file.starts_with("# The patterns kept for cas under the rule 15,7,6,10,13,12,2,8,14,11,5,4,3,9,1,0:\n# a")
        );
        let lines: Vec<&str> = file.lines().filter(|line| !line.starts_with('#')).collect();
        assert_eq!(
            lines,
            [
                "sort\tpattern\tperiod\tmoves\tname\tnote",
                "spaceship\tbobo$obo\t7328092\t0,-8\tThe slowest\teight cells up in a period",
            ]
        );
        let other = collection.text_of(&rule("single-rotation")).unwrap();
        assert!(other.contains("0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15 (Single rotation):\n"));
        assert!(other.ends_with("sort\tpattern\tperiod\tmoves\tname\tnote\nstill life\t2o$2o\t1\t\t\t\n"));
        assert_eq!(collection.text_of(&rule("critters")), None);
        // Read again, it is the same collection, but for the tab and the line break that a
        // line cannot hold.
        let read = Collection::parse(&file, &slow).unwrap();
        assert_eq!(read.all()[0].name, "The slowest");
        assert_eq!(read.all()[0].rule, slow);
        assert_eq!(Collection::parse(&read.text_of(&slow).unwrap(), &slow).unwrap(), read);
    }

    #[test]
    fn a_file_is_read_whole_or_not_at_all() {
        let critters = rule("critters");
        // Comments, the names of the columns, a line with nothing after the period, and the
        // same pattern twice.
        let file = "# kept\n\noscillator\to\t4\nStill Life\t2o$2o\t2\t\tA block\noscillator\to\t4\t\tagain";
        let collection = Collection::parse(file, &critters).unwrap();
        assert_eq!(collection.all().len(), 2);
        assert_eq!((collection.all()[1].sort, collection.all()[1].name.as_str()), (Sort::StillLife, "A block"));
        assert!(collection.all().iter().all(|kept| kept.rule == critters));
        // A file from when the day a pattern was kept had a column: read by the names of its
        // columns, and written again without the day.
        let older = "sort\tpattern\tperiod\tmoves\tname\tadded\tnote\n\
                     spaceship\tb2o2$b2o\t12\t2,0\tLightest\t2026-10-04\ta note";
        let read = Collection::parse(older, &rule("single-rotation")).unwrap();
        let ship = &read.all()[0];
        assert_eq!(
            (ship.name.as_str(), ship.note.as_str(), ship.period, ship.moves),
            ("Lightest", "a note", 12, (2, 0))
        );
        let written = read.text_of(&rule("single-rotation")).unwrap();
        assert!(!written.contains("2026") && !written.contains("added"));
        // What is wrong is said, with its line.
        let wrong = |line: &str| Collection::parse(&format!("oscillator\to\t4\n{line}"), &critters).unwrap_err();
        assert_eq!(wrong("gun\to\t4"), "line 2: \"gun\" is no sort of pattern");
        assert_eq!(wrong("spaceship\to\tsoon"), "line 2: \"soon\" is no period");
        assert_eq!(wrong("spaceship\to\t4\tup"), "line 2: \"up\" is no way to move");
        assert_eq!(wrong("spaceship\t\t4"), "line 2: a pattern has cells");
        assert!(wrong("spaceship\t2x\t4").starts_with("line 2: "));
        // The sorts by what a pattern does, and by name, one or several.
        assert_eq!("still-lifes".parse::<Sort>(), Ok(Sort::StillLife));
        assert_eq!("Spaceships".parse::<Sort>(), Ok(Sort::Spaceship));
        assert!("ships".parse::<Sort>().is_err());
        assert_eq!(Sort::of((0, 0), true), Sort::StillLife);
        assert_eq!(Sort::of((0, 0), false), Sort::Oscillator);
        assert_eq!(Sort::of((2, 0), false), Sort::Spaceship);
        assert!(usual_folder().ends_with("patterns") && usual_folder().with_file_name("Cargo.toml").exists());
    }

    #[test]
    fn the_folder_is_written_and_read_again() {
        let folder = std::env::temp_dir().join(format!("cas-collection-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        // No folder yet: nothing was kept. The folder is made when something is.
        assert_eq!(Collection::read(&folder).unwrap(), Collection::default());
        let (rotation, critters) = (rule("single-rotation"), rule("critters"));
        let mut collection = Collection::default();
        collection.keep(Kept::new(&rotation, Sort::Spaceship, &cells("b2o2$b2o"), 12, (2, 0))).unwrap();
        collection.keep(Kept::new(&critters, Sort::Oscillator, &cells("o"), 4, (0, 0))).unwrap();
        collection.write(&folder).unwrap();
        assert_eq!(Collection::read(&folder).unwrap(), collection);
        let names = |folder: &Path| {
            let mut names: Vec<String> = fs::read_dir(folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(names(&folder), ["0283156749abcdef.tsv", "fed3b56179a2c480.tsv"]);
        // Only the file of a rule whose patterns changed is written again; a file that is
        // not a rule's is left alone and not read.
        fs::write(folder.join("notes.txt"), "mine").unwrap();
        let written = fs::metadata(folder.join("fed3b56179a2c480.tsv")).unwrap().modified().unwrap();
        collection.keep(Kept::new(&rotation, Sort::StillLife, &cells("2o$2o"), 1, (0, 0))).unwrap();
        collection.write(&folder).unwrap();
        assert_eq!(fs::metadata(folder.join("fed3b56179a2c480.tsv")).unwrap().modified().unwrap(), written);
        let read = Collection::read(&folder).unwrap();
        assert_eq!(read.all().len(), 3);
        assert_eq!(Collection::read_file(&Collection::file(&folder, &rotation), &rotation).unwrap().all().len(), 2);
        // The file of a rule with nothing kept any more goes.
        assert!(collection.forget(&critters, &cells("o")));
        collection.write(&folder).unwrap();
        assert_eq!(names(&folder), ["0283156749abcdef.tsv", "notes.txt"]);
        assert_eq!(
            Collection::read_file(&Collection::file(&folder, &critters), &critters).unwrap(),
            Collection::default()
        );
        // A file of a rule's that cannot be read is an error, with its name; so is a file
        // named like a rule's that is no rule's, or not in lower case: it would make a rule
        // twice over. A file named otherwise is not a rule's and is left alone.
        fs::write(folder.join("0283156749abcdef.tsv"), "oscillator\to\tsoon\n").unwrap();
        let wrong = Collection::read(&folder).unwrap_err();
        assert!(wrong.contains("0283156749abcdef.tsv: line 1: \"soon\" is no period"), "{wrong}");
        fs::remove_file(folder.join("0283156749abcdef.tsv")).unwrap();
        assert_eq!(Collection::read(&folder).unwrap(), Collection::default());
        for name in ["0283156749ABCDEF.tsv", "0283156749abcdee.tsv"] {
            fs::write(folder.join(name), "oscillator\to\t4\n").unwrap();
            let wrong = Collection::read(&folder).unwrap_err();
            assert!(wrong.contains(name) && wrong.contains("not a rule's file"), "{wrong}");
            fs::remove_file(folder.join(name)).unwrap();
        }
        fs::write(folder.join("some-rule.tsv"), "oscillator\to\t4\n").unwrap();
        assert_eq!(Collection::read(&folder).unwrap(), Collection::default());
        let _ = fs::remove_dir_all(&folder);
    }
}
