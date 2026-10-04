//! The collection: the patterns that were kept, rule by rule.
//!
//! A pattern that comes back to its shape is worth keeping: a spaceship, an oscillator, a
//! still life. The kept ones lie in a plain text file next to the library's, a pattern to a
//! line, which is meant to be read, edited by hand and kept under version control, and which
//! the app, the search and whatever else looks at rules may all add to:
//!
//! ```text
//! rule             sort        pattern   period  moves  name  added       note
//! single-rotation  spaceship   b2o2$b2o  12      2,0          2026-10-04  the lightest
//! single-rotation  still life  2o$2o     1                    2026-10-04
//! ```
//!
//! The columns are separated by tabs, one between any two (they are drawn apart here). A
//! pattern is written as the app writes one: run-length encoded, from a corner of the blocks
//! the next step rewrites, at the start of the vacuum's cycle. It is the form its kind is
//! filed under ([`Motion::canonical`](crate::pattern::Motion)), so that a kind is kept once
//! whichever way it was found lying.

use std::{fmt, fs, io, path::Path, path::PathBuf, str::FromStr};

use crate::{
    library,
    pattern::{Cell, from_rle, settled, to_rle},
    rules::{BlockRule, PRESETS},
};

/// What the file begins with: what it is, and its columns by name.
const HEADING: &str = "\
# The patterns kept for cas, a pattern to a line, with tabs in between: the rule it is a
# pattern of (its table, or the id of a built-in rule), what it is (a spaceship, an
# oscillator or a still life), the pattern (run-length encoded, from a corner of the blocks
# the next step rewrites, at the start of the vacuum's cycle), its period, how far it moves
# in a period, a name, the day it was kept, and a note.
rule\tsort\tpattern\tperiod\tmoves\tname\tadded\tnote
";

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

    fn from_str(text: &str) -> Result<Self, String> {
        let wanted: String = text.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
        let named = |sort: &Sort| sort.name().replace(' ', "") == wanted;
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
    /// The day it was kept, as it was written down then.
    pub added: String,
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
            added: String::new(),
            note: String::new(),
        }
    }
}

/// The patterns that were kept, in the order they were.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collection {
    kept: Vec<Kept>,
}

impl Collection {
    /// The collection a file describes. A line that cannot be read is an error, with its
    /// number: a file read in part would lose the rest when it is written again.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut collection = Self::default();
        for (number, text) in text.lines().enumerate() {
            if text.trim().is_empty() || text.starts_with('#') {
                continue;
            }
            let mut fields = text.split('\t').map(str::trim);
            let mut field = || fields.next().unwrap_or_default();
            let (rule, sort, pattern, period, moves) = (field(), field(), field(), field(), field());
            let (name, added, note) = (field(), field(), field());
            // The names of the columns.
            if rule == "rule" {
                continue;
            }
            let wrong = |error: String| format!("line {}: {error}", number + 1);
            let rule: BlockRule = rule.parse().map_err(wrong)?;
            let sort: Sort = sort.parse().map_err(wrong)?;
            let cells = from_rle(pattern).map_err(wrong)?;
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
            let kept = Kept {
                name: name.to_string(),
                added: added.to_string(),
                note: note.to_string(),
                ..Kept::new(&rule, sort, &cells, period, moves)
            };
            // The same pattern twice is the same pattern.
            let _ = collection.keep(kept);
        }
        Ok(collection)
    }

    /// The collection of a file. Where there is no file yet, nothing was kept.
    pub fn read(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Writes the file, whole or not at all, as the library writes its own.
    pub fn write(&self, path: &Path) -> io::Result<()> {
        library::write_whole(path, &self.to_string())
    }

    pub fn all(&self) -> &[Kept] {
        &self.kept
    }

    /// The patterns of one sort kept under a rule, in the order they were kept.
    pub fn of<'a>(&'a self, rule: &'a BlockRule, sort: Sort) -> impl Iterator<Item = &'a Kept> {
        self.kept.iter().filter(move |kept| kept.sort == sort && kept.rule == *rule)
    }

    /// Where a pattern of a rule is, if it was kept. The cells are those of the form its
    /// kind is filed under.
    pub fn find(&self, rule: &BlockRule, cells: &[Cell]) -> Option<usize> {
        let cells = settled(cells);
        self.kept.iter().position(|kept| kept.rule == *rule && kept.cells == cells)
    }

    /// Keeps a pattern. One that is kept already is not kept twice: then the place it has
    /// comes back as the error.
    pub fn keep(&mut self, kept: Kept) -> Result<usize, usize> {
        if let Some(known) = self.find(&kept.rule, &kept.cells) {
            return Err(known);
        }
        self.kept.push(Kept { cells: settled(&kept.cells), ..kept });
        Ok(self.kept.len() - 1)
    }

    /// Lets go of a pattern of a rule. True if it was kept.
    pub fn forget(&mut self, rule: &BlockRule, cells: &[Cell]) -> bool {
        let found = self.find(rule, cells);
        if let Some(index) = found {
            self.kept.remove(index);
        }
        found.is_some()
    }
}

/// The file: the patterns in the order they were kept.
impl fmt::Display for Collection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(HEADING)?;
        let line = |text: &str| text.split(['\t', '\n', '\r']).collect::<Vec<_>>().join(" ").trim().to_string();
        for kept in &self.kept {
            // A built-in rule goes by its id, which says more than its table.
            let rule = match PRESETS.iter().find(|preset| preset.table == *kept.rule.table()) {
                Some(preset) => preset.id.to_string(),
                None => kept.rule.to_string(),
            };
            let moves = match kept.moves {
                (0, 0) => String::new(),
                (dx, dy) => format!("{dx},{dy}"),
            };
            let pattern = to_rle(&kept.cells);
            let (name, note) = (line(&kept.name), line(&kept.note));
            writeln!(
                f,
                "{rule}\t{}\t{pattern}\t{}\t{moves}\t{name}\t{}\t{note}",
                kept.sort,
                kept.period,
                line(&kept.added)
            )?;
        }
        Ok(())
    }
}

/// The file of the collection where no other is named: `patterns.tsv`, next to the library's
/// own file.
pub fn usual_file() -> PathBuf {
    library::usual_file().with_file_name("patterns.tsv")
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
        assert_eq!(collection.keep(Kept::new(&critters, Sort::Oscillator, &ship.cells, 4, (0, 0))), Ok(1));
        assert_eq!(collection.keep(Kept::new(&rotation, Sort::StillLife, &cells("2o$2o"), 1, (0, 0))), Ok(2));
        // Each rule has its own, sort by sort.
        let of = |collection: &Collection, rule: &BlockRule, sort| collection.of(rule, sort).count();
        assert_eq!(of(&collection, &rotation, Sort::Spaceship), 1);
        assert_eq!((of(&collection, &rotation, Sort::StillLife), of(&collection, &rotation, Sort::Oscillator)), (1, 0));
        assert_eq!((of(&collection, &critters, Sort::Oscillator), of(&collection, &critters, Sort::Spaceship)), (1, 0));
        // What was kept can be let go of.
        assert!(collection.forget(&rotation, &moved) && !collection.forget(&rotation, &moved));
        assert_eq!((collection.all().len(), of(&collection, &rotation, Sort::Spaceship)), (2, 0));
    }

    #[test]
    fn the_file_says_what_was_kept() {
        let slow = rule("15,7,6,10,13,12,2,8,14,11,5,4,3,9,1,0");
        let mut collection = Collection::default();
        let mut ship = Kept::new(&slow, Sort::Spaceship, &cells("bobo$obo"), 7_328_092, (0, -8));
        ship.added = "2026-10-04".to_string();
        ship.name = "The\tslowest".to_string();
        ship.note = "eight cells up\nin a period".to_string();
        collection.keep(ship).unwrap();
        collection.keep(Kept::new(&rule("single-rotation"), Sort::StillLife, &cells("2o$2o"), 1, (0, 0))).unwrap();
        let file = collection.to_string();
        let lines: Vec<&str> = file.lines().filter(|line| !line.starts_with('#')).collect();
        assert_eq!(
            lines,
            [
                "rule\tsort\tpattern\tperiod\tmoves\tname\tadded\tnote",
                "15,7,6,10,13,12,2,8,14,11,5,4,3,9,1,0\tspaceship\tbobo$obo\t7328092\t0,-8\tThe slowest\t2026-10-04\teight cells up in a period",
                "single-rotation\tstill life\t2o$2o\t1\t\t\t\t",
            ]
        );
        // Read again, it is the same collection, but for the tab and the line break that a
        // line cannot hold.
        let read = Collection::parse(&file).unwrap();
        assert_eq!(read.all()[0].name, "The slowest");
        assert_eq!(read.all()[1], collection.all()[1]);
        assert_eq!(Collection::parse(&read.to_string()).unwrap(), read);
    }

    #[test]
    fn a_file_is_read_whole_or_not_at_all() {
        // Comments, the names of the columns, a line with nothing after the period, and the
        // same pattern twice.
        let file = "# kept\n\nrule\tsort\ncritters\toscillator\to\t4\nCritters\tStill Life\t2o$2o\t2\t\tA block\n\
                    critters\toscillator\to\t4\t\tagain";
        let collection = Collection::parse(file).unwrap();
        assert_eq!(collection.all().len(), 2);
        assert_eq!((collection.all()[1].sort, collection.all()[1].name.as_str()), (Sort::StillLife, "A block"));
        // What is wrong is said, with its line.
        let wrong = |line: &str| Collection::parse(&format!("critters\toscillator\to\t4\n{line}")).unwrap_err();
        assert_eq!(wrong("0,1,2\tspaceship\to\t4"), "line 2: a rule has 16 entries, found 3");
        assert_eq!(wrong("critters\tgun\to\t4"), "line 2: \"gun\" is no sort of pattern");
        assert_eq!(wrong("critters\tspaceship\to\tsoon"), "line 2: \"soon\" is no period");
        assert_eq!(wrong("critters\tspaceship\to\t4\tup"), "line 2: \"up\" is no way to move");
        assert_eq!(wrong("critters\tspaceship\t\t4"), "line 2: a pattern has cells");
        assert!(wrong("critters\tspaceship\t2x\t4").starts_with("line 2: "));
        // The sorts by what a pattern does.
        assert_eq!(Sort::of((0, 0), true), Sort::StillLife);
        assert_eq!(Sort::of((0, 0), false), Sort::Oscillator);
        assert_eq!(Sort::of((2, 0), false), Sort::Spaceship);
        assert!(usual_file().ends_with("patterns.tsv") && usual_file().with_file_name("Cargo.toml").exists());
    }

    #[test]
    fn the_file_is_written_and_read_again() {
        let folder = std::env::temp_dir().join(format!("cas-collection-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("patterns.tsv");
        assert_eq!(Collection::read(&path).unwrap(), Collection::default());
        let mut collection = Collection::default();
        collection.keep(Kept::new(&rule("single-rotation"), Sort::Spaceship, &cells("b2o2$b2o"), 12, (2, 0))).unwrap();
        collection.write(&path).unwrap();
        assert_eq!(Collection::read(&path).unwrap(), collection);
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&folder);
    }
}
