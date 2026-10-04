//! The rule library: the rules that go by a name.
//!
//! There are those that come with the program ([`PRESETS`]), and those that were kept. The
//! kept ones lie in a plain text file, a rule to a line, which is meant to be read, edited by
//! hand and kept under version control:
//!
//! ```text
//! rule                                   name      pinned  tags        added       note
//! critters                                         *
//! 0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15  Lone gun          gun, found  2026-10-04  fires four ways
//! ```
//!
//! The columns are separated by tabs, one between any two (they are drawn apart here). A rule
//! is written as its table, as the id of a built-in rule or as Morita's number. A line about
//! a built-in rule only says whether it is pinned.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    families::{self, Constraint},
    rules::{BlockRule, PRESETS, Source},
};

/// The built-in rules that are pinned until a library says otherwise.
pub const PINNED: [&str; 6] = ["single-rotation", "critters", "bbm", "tron", "espca-01c5ef", "four-way-gun"];

/// What the file begins with: what it is, and its columns by name.
const HEADING: &str = "\
# The rules kept for cas, a rule to a line, with tabs in between: the rule (its table, or
# the id of a built-in rule, or Morita's number), its name, a * if it is pinned to the rule
# menu, its tags, the day it was kept, and a note. A line about a built-in rule only says
# whether it is pinned.
rule\tname\tpinned\ttags\tadded\tnote
";

/// A rule of the library.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub rule: BlockRule,
    pub name: String,
    /// What it does, or what there is to remember about it.
    pub note: String,
    pub tags: Vec<String>,
    /// The day it was kept, as it was written down then.
    pub added: String,
    /// It is one of the few the rule menu offers.
    pub pinned: bool,
    /// Where a built-in rule is from. None for a rule that was kept: only those can be
    /// renamed, written about and forgotten.
    pub source: Option<Source>,
    /// The canonical form of the rule: the same for every rule of its world.
    world: BlockRule,
    /// What can be searched for: its words, and the names of the properties it has.
    words: String,
}

impl Entry {
    fn new(rule: BlockRule, name: &str, source: Option<Source>) -> Self {
        let world = rule.canonical();
        let mut entry = Self {
            rule,
            name: line(name),
            note: String::new(),
            tags: Vec::new(),
            added: String::new(),
            pinned: false,
            source,
            world,
            words: String::new(),
        };
        entry.index();
        entry
    }

    /// Whether it was kept, as opposed to having come with the program.
    pub fn kept(&self) -> bool {
        self.source.is_none()
    }

    /// Puts together what can be searched for.
    fn index(&mut self) {
        let from = match self.source {
            Some(Source::Collections) => "collections built-in",
            Some(Source::Morita) => "morita book built-in",
            Some(Source::Search) => "search found built-in",
            None => "kept",
        };
        let number = self.rule.espca().map(|number| format!("espca-{number}")).unwrap_or_default();
        self.words = format!(
            "{} {} {} {from} {} {number} {}",
            self.name,
            self.tags.join(" "),
            self.note,
            self.rule,
            properties(&self.rule).join(" ")
        )
        .to_lowercase();
    }
}

/// The rules that go by a name: the built-in ones in their order, then the kept ones in the
/// order they were kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    entries: Vec<Entry>,
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

impl Library {
    /// The library before anything was kept: the built-in rules, the usual ones pinned.
    pub fn new() -> Self {
        let mut library = Self::built_in();
        for entry in &mut library.entries {
            entry.pinned =
                PRESETS.iter().any(|preset| preset.table == *entry.rule.table() && PINNED.contains(&preset.id));
        }
        library
    }

    fn built_in() -> Self {
        let entries = PRESETS.iter().map(|preset| {
            let mut entry = Entry::new(preset.rule(), preset.name, Some(preset.source));
            entry.note = preset.blurb.to_string();
            entry.index();
            entry
        });
        Self { entries: entries.collect() }
    }

    /// The library a file describes: the built-in rules, pinned as it says, and the rules it
    /// keeps. A line that cannot be read is an error, with its number: a file read in part
    /// would lose the rest when it is written again.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut library = Self::built_in();
        for (number, text) in text.lines().enumerate() {
            if text.trim().is_empty() || text.starts_with('#') {
                continue;
            }
            let mut fields = text.split('\t').map(str::trim);
            let mut field = || fields.next().unwrap_or_default();
            let (rule, name, pinned, tags, added, note) = (field(), field(), field(), field(), field(), field());
            // The names of the columns.
            if rule == "rule" {
                continue;
            }
            let rule: BlockRule = rule.parse().map_err(|error| format!("line {}: {error}", number + 1))?;
            let pinned = !pinned.is_empty();
            match library.of(&rule).filter(|&index| !library.entries[index].kept()) {
                Some(built_in) => library.entries[built_in].pinned = pinned,
                None => {
                    let mut entry = Entry::new(rule, if name.is_empty() { "Unnamed" } else { name }, None);
                    entry.pinned = pinned;
                    entry.tags = tags_of(tags);
                    entry.added = added.to_string();
                    entry.note = note.to_string();
                    entry.index();
                    library.entries.push(entry);
                }
            }
        }
        Ok(library)
    }

    /// The library of a file. Where there is no file yet, it is the library before anything
    /// was kept.
    pub fn read(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Writes the file, whole or not at all: beside itself first, and then in its place, so
    /// that whoever reads it meanwhile reads all of the old one or all of the new.
    pub fn write(&self, path: &Path) -> io::Result<()> {
        let mut beside = path.as_os_str().to_owned();
        beside.push(".new");
        fs::write(&beside, self.to_string())?;
        fs::rename(&beside, path)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry with this very table.
    pub fn of(&self, rule: &BlockRule) -> Option<usize> {
        self.entries.iter().position(|entry| entry.rule == *rule)
    }

    /// The entry of the same world: the rule itself, or one that is it turned, mirrored, or
    /// begun at another generation of its vacuum's cycle.
    pub fn twin(&self, rule: &BlockRule) -> Option<usize> {
        self.of(rule).or_else(|| {
            let world = rule.canonical();
            self.entries.iter().position(|entry| entry.world == world)
        })
    }

    /// Keeps a rule under a name. A rule whose world is in the library already is not kept
    /// twice: then the entry it has comes back as the error.
    pub fn keep(&mut self, rule: BlockRule, name: &str, added: &str) -> Result<usize, usize> {
        if let Some(known) = self.twin(&rule) {
            return Err(known);
        }
        let mut entry = Entry::new(rule, name, None);
        entry.added = line(added);
        self.entries.push(entry);
        Ok(self.entries.len() - 1)
    }

    /// A name no rule has yet: `stem` with the first number that is free.
    pub fn unused(&self, stem: &str) -> String {
        let taken = |name: &String| self.entries.iter().any(|entry| entry.name == *name);
        (1..).map(|number| format!("{stem} {number}")).find(|name| !taken(name)).unwrap()
    }

    /// Forgets a rule that was kept. The built-in ones stay.
    pub fn forget(&mut self, index: usize) -> bool {
        let kept = self.entries.get(index).is_some_and(Entry::kept);
        if kept {
            self.entries.remove(index);
        }
        kept
    }

    pub fn pin(&mut self, index: usize, pinned: bool) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.pinned = pinned;
        }
    }

    /// Changes what is written of a rule that was kept: its name, its tags (separated by
    /// commas) and its note. Nothing happens to a built-in rule.
    pub fn rename(&mut self, index: usize, name: &str) {
        self.rewrite(index, |entry| entry.name = line(name));
    }

    pub fn tag(&mut self, index: usize, tags: &str) {
        self.rewrite(index, |entry| entry.tags = tags_of(tags));
    }

    pub fn annotate(&mut self, index: usize, note: &str) {
        self.rewrite(index, |entry| entry.note = line(note));
    }

    fn rewrite(&mut self, index: usize, change: impl FnOnce(&mut Entry)) {
        if let Some(entry) = self.entries.get_mut(index).filter(|entry| entry.kept()) {
            change(entry);
            entry.index();
        }
    }

    /// The entries a search leaves, in their order: those that have every word of `words`
    /// somewhere in their name, tags, note, origin, table, or among the names of their
    /// properties (`conserving`, `half-turn`, ...), and that have all of `properties`.
    pub fn matching(&self, words: &str, properties: &[Constraint]) -> Vec<usize> {
        let words: Vec<String> = words.split_whitespace().map(str::to_lowercase).collect();
        let matches = |entry: &Entry| {
            words.iter().all(|word| entry.words.contains(word.as_str()))
                && properties.iter().all(|property| property.holds(&entry.rule))
        };
        (0..self.entries.len()).filter(|&index| matches(&self.entries[index])).collect()
    }
}

/// The file: the built-in rules that are pinned, then the rules that were kept.
impl fmt::Display for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(HEADING)?;
        for entry in &self.entries {
            let pinned = if entry.pinned { "*" } else { "" };
            match PRESETS.iter().find(|preset| !entry.kept() && preset.table == *entry.rule.table()) {
                Some(preset) if entry.pinned => writeln!(f, "{}\t\t{pinned}", preset.id)?,
                Some(_) => {}
                None => {
                    let tags = entry.tags.join(", ");
                    writeln!(f, "{}\t{}\t{pinned}\t{tags}\t{}\t{}", entry.rule, entry.name, entry.added, entry.note)?;
                }
            }
        }
        Ok(())
    }
}

/// The file of the library where no other is named: `rules.tsv` at the root of the repository
/// the program was built from, wherever it is run. The app and the search share it.
pub fn usual_file() -> PathBuf {
    let core = Path::new(env!("CARGO_MANIFEST_DIR"));
    core.ancestors().nth(2).unwrap_or(core).join("rules.tsv")
}

/// Today's date as the library writes it down: year, month and day.
pub fn today() -> String {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() / 86_400) as i64;
    // The civil date of a day counted from 1970, after Howard Hinnant.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The properties a rule has, of those that go by a name: `conserving`, `half-turn` and the
/// like, as a family is asked for.
pub fn properties(rule: &BlockRule) -> Vec<&'static str> {
    families::named().filter(|(_, property)| property.holds(rule)).map(|(name, _)| name).collect()
}

/// A text as a field of a line holds it: without tabs and line breaks, and without space
/// around it.
fn line(text: &str) -> String {
    text.split(['\t', '\n', '\r']).collect::<Vec<_>>().join(" ").trim().to_string()
}

/// Tags as they are written, with commas in between.
fn tags_of(text: &str) -> Vec<String> {
    line(text).split(',').map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(text: &str) -> BlockRule {
        text.parse().unwrap()
    }

    #[test]
    fn the_usual_file_is_the_repository_s() {
        let file = usual_file();
        assert!(file.ends_with("rules.tsv"));
        // Next to the workspace's manifest, and written without a detour.
        assert!(file.with_file_name("Cargo.toml").exists() && file.with_file_name("crates").exists());
        assert!(!file.to_string_lossy().contains(".."));
    }

    #[test]
    fn today_is_a_date() {
        let today = today();
        let parts: Vec<u32> = today.split('-').map(|part| part.parse().unwrap()).collect();
        assert_eq!((today.len(), parts.len()), (10, 3));
        assert!(parts[0] >= 2026 && (1..=12).contains(&parts[1]) && (1..=31).contains(&parts[2]), "{today}");
    }

    #[test]
    fn the_library_begins_with_the_built_in_rules() {
        let library = Library::new();
        assert_eq!(library.entries().len(), PRESETS.len());
        assert!(library.entries().iter().all(|entry| !entry.kept() && !entry.note.is_empty()));
        // The usual ones are pinned, and every one of those is a rule there is.
        let pinned: Vec<&str> =
            library.entries().iter().filter(|entry| entry.pinned).map(|entry| &*entry.name).collect();
        assert_eq!(pinned.len(), PINNED.len());
        assert_eq!(pinned[..2], ["Single rotation", "Critters"]);
        // A file says which are pinned: one that says nothing pins none.
        let unpinned = Library::parse("").unwrap();
        assert!(unpinned.entries().iter().all(|entry| !entry.pinned));
        assert_eq!(unpinned.entries().len(), PRESETS.len());
    }

    #[test]
    fn a_rule_is_kept_once_whatever_form_it_comes_in() {
        let mut library = Library::new();
        let gun = rule("0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15");
        let kept = library.keep(gun.clone(), "A\tgun\n", "2026-10-04").unwrap();
        assert_eq!(kept, PRESETS.len());
        assert_eq!((library.entries()[kept].name.as_str(), library.entries()[kept].kept()), ("A gun", true));
        assert_eq!((library.of(&gun), library.twin(&gun)), (Some(kept), Some(kept)));
        // The same rule again, and the rule in canonical form, which is another table of the
        // same world: both are there already.
        assert_eq!(library.keep(gun.clone(), "Again", ""), Err(kept));
        let twin = gun.canonical();
        assert_ne!(twin, gun);
        assert_eq!((library.of(&twin), library.twin(&twin)), (None, Some(kept)));
        assert_eq!(library.keep(twin, "Its twin", ""), Err(kept));
        // A built-in rule is in the library from the start.
        let critters = library.of(&rule("critters")).unwrap();
        assert_eq!(library.keep(rule("critters"), "Mine", ""), Err(critters));
        // What was kept can be forgotten, and what is built in cannot.
        assert!(!library.forget(critters));
        assert!(library.forget(kept));
        assert_eq!((library.entries().len(), library.twin(&gun)), (PRESETS.len(), None));
    }

    #[test]
    fn the_file_says_what_was_kept_and_what_is_pinned() {
        let mut library = Library::new();
        let kept = library.keep(rule("0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15"), "A gun", "2026-10-04").unwrap();
        library.tag(kept, " gun,  half-turn , ");
        library.annotate(kept, "fires\tfour ways");
        library.pin(kept, true);
        library.pin(library.of(&rule("critters")).unwrap(), false);
        // Only kept rules are written about: a built-in one keeps its name and its note.
        let tron = library.of(&rule("tron")).unwrap();
        library.rename(tron, "Mine");
        library.annotate(tron, "mine");
        assert_eq!(library.entries()[tron].name, "Tron");
        let file = library.to_string();
        let lines: Vec<&str> = file.lines().filter(|line| !line.starts_with('#')).collect();
        assert_eq!(
            lines,
            [
                "rule\tname\tpinned\ttags\tadded\tnote",
                "single-rotation\t\t*",
                "bbm\t\t*",
                "tron\t\t*",
                "espca-01c5ef\t\t*",
                "four-way-gun\t\t*",
                "0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15\tA gun\t*\tgun, half-turn\t2026-10-04\tfires four ways",
            ]
        );
        // Read again, it is the same library.
        assert_eq!(Library::parse(&file).unwrap(), library);
    }

    #[test]
    fn a_file_is_read_as_far_as_it_makes_sense() {
        // A rule by its id, by Morita's number and by its table; a line with nothing after the
        // name; and a kept rule without a name.
        let file = "# a comment\n\nrule\tname\ncritters\t\t*\nespca-04cadf\tignored\n\
                    0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15\tA gun\n\
                    0,4,8,3,1,5,6,7,2,9,10,11,12,13,14,15\t\t*\ttwo, tags";
        let library = Library::parse(file).unwrap();
        let pinned: Vec<&str> =
            library.entries().iter().filter(|entry| entry.pinned).map(|entry| &*entry.name).collect();
        assert_eq!(pinned, ["Critters", "Unnamed"]);
        let kept: Vec<&Entry> = library.entries().iter().filter(|entry| entry.kept()).collect();
        assert_eq!(kept.len(), 2);
        assert_eq!((kept[0].name.as_str(), kept[0].pinned, kept[0].tags.len()), ("A gun", false, 0));
        assert_eq!(kept[1].tags, ["two", "tags"]);
        // What is no rule is said, with its line.
        assert_eq!(
            Library::parse("critters\n0,1,2\tHalf a rule").unwrap_err(),
            "line 2: a rule has 16 entries, found 3"
        );
    }

    #[test]
    fn the_file_is_written_whole_and_read_again() {
        let folder = std::env::temp_dir().join(format!("cas-library-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("rules.tsv");
        // Where there is no file yet, nothing was kept.
        assert_eq!(Library::read(&path).unwrap(), Library::new());
        // A rule kept without a name of its own gets one that is free.
        let mut library = Library::new();
        let name = library.unused("Unnamed");
        library.keep(rule("0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15"), &name, "2026-10-04").unwrap();
        assert_eq!((name.as_str(), library.unused("Unnamed").as_str()), ("Unnamed 1", "Unnamed 2"));
        library.write(&path).unwrap();
        assert_eq!(Library::read(&path).unwrap(), library);
        // Nothing is left beside the file.
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
        // A file that is no library is said to be none.
        fs::write(&path, "0,1,2\tHalf a rule\n").unwrap();
        assert_eq!(Library::read(&path).unwrap_err(), "line 1: a rule has 16 entries, found 3");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn rules_are_found_by_their_words_and_their_properties() {
        let mut library = Library::new();
        let kept = library.keep(rule("0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15"), "A gun", "2026-10-04").unwrap();
        library.tag(kept, "Guns");
        library.annotate(kept, "Fires four ways");
        let names = |found: Vec<usize>| -> Vec<String> {
            found.into_iter().map(|index| library.entries()[index].name.clone()).collect()
        };
        // Nothing asked for is everything.
        assert_eq!(library.matching("", &[]).len(), library.entries().len());
        // Words are looked for in the name, the tags and the note, whatever their case, and
        // all of them have to be there.
        assert_eq!(names(library.matching("gun", &[])), ["ESPCA-09457f", "Four-way gun", "A gun"]);
        assert_eq!(names(library.matching("GUNS", &[])), ["A gun"]);
        assert_eq!(names(library.matching("four fires", &[])), ["A gun"]);
        assert_eq!(names(library.matching("kept", &[])), ["A gun"]);
        assert!(library.matching("gun nowhere", &[]).is_empty());
        // A rule is found by its table and by Morita's number, and by where it is from.
        assert_eq!(names(library.matching("0,2,8,6", &[])), ["A gun"]);
        assert_eq!(names(library.matching("espca-f6b580", &[])), ["Four-way gun"]);
        assert_eq!(library.matching("morita", &[]).len(), 9);
        // And by what it has: as a word, or as a property asked for.
        let conserving = library.matching("conserving", &[]);
        assert_eq!(conserving, library.matching("", &[Constraint::Conserving]));
        assert!(conserving.len() > 5 && conserving.len() < library.entries().len());
        assert!(names(conserving).contains(&"Critters".to_string()));
        // Both at once: the guns that keep their cells are none of the built-in ones.
        assert_eq!(names(library.matching("gun", &[Constraint::Conserving])), ["A gun"]);
    }
}
