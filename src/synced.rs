//! Files kept in step with what the program holds of them.
//!
//! The rule library is a plain text file and the collection of patterns a folder of them,
//! which the app, the search and an editor may all write. So the app holds what the files
//! have, reads them again whenever something else wrote them, and writes what changed whole
//! at every change, on top of the files as they are then: nothing that was put there
//! meanwhile is written over.

use std::{
    fs, io,
    ops::Deref,
    path::{Path, PathBuf},
    time::SystemTime,
};

use cas_core::{collection::Collection, library::Library};

/// What lies at a path of its own: in a file, or in a folder of files.
pub trait Stored: Default {
    /// What is there; of a file or folder that is not there yet, what there is before one.
    fn read(path: &Path) -> Result<Self, String>;
    /// Writes what changed since it was read or written.
    fn write(&mut self, path: &Path) -> io::Result<()>;
}

impl Stored for Library {
    fn read(path: &Path) -> Result<Self, String> {
        Library::read(path)
    }

    fn write(&mut self, path: &Path) -> io::Result<()> {
        Library::write(self, path)
    }
}

impl Stored for Collection {
    fn read(path: &Path) -> Result<Self, String> {
        Collection::read(path)
    }

    fn write(&mut self, path: &Path) -> io::Result<()> {
        Collection::write(self, path)
    }
}

/// What tells a file that was written from one that was not: when it was written, and how
/// long it is; of a folder, that of each file in it, by name. None for what is not there.
type Stamp = Option<Vec<(String, SystemTime, u64)>>;

fn stamp(path: &Path) -> Stamp {
    let found = fs::metadata(path).ok()?;
    if !found.is_dir() {
        return Some(vec![(String::new(), found.modified().ok()?, found.len())]);
    }
    let mut files: Vec<(String, SystemTime, u64)> = fs::read_dir(path)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            // Through a link, to the file itself.
            let file = fs::metadata(entry.path()).ok()?;
            file.is_file()
                .then(|| Some((entry.file_name().to_string_lossy().into_owned(), file.modified().ok()?, file.len())))?
        })
        .collect();
    files.sort();
    Some(files)
}

/// Something and the file or folder it lies in. Without one it is kept nowhere: a scripted
/// run is to find the same every time, and to leave the files alone.
pub struct Synced<T> {
    held: T,
    path: Option<PathBuf>,
    /// The file, or the folder's files, as they were when last read or written here.
    stamp: Stamp,
    /// Why the file could not be read, if it could not: it is left as it is then, and
    /// nothing is written.
    unreadable: Option<String>,
    /// Why the file could not be written, the last time it was to be.
    unwritten: Option<String>,
}

impl<T> Deref for Synced<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.held
    }
}

impl<T: Stored> Synced<T> {
    /// What the file or folder at `path` holds.
    pub fn at(path: Option<PathBuf>) -> Self {
        let mut synced = Self { held: T::default(), path, stamp: None, unreadable: None, unwritten: None };
        synced.read_again();
        synced
    }

    /// Reads the file, or the folder's files, if they are not as they were last seen here:
    /// something else wrote them. True if they were read. A file that cannot be read changes
    /// nothing, and is said to be wrong for as long as it is.
    pub fn read_again(&mut self) -> bool {
        let Some(path) = &self.path else {
            return false;
        };
        let stamp = stamp(path);
        if stamp == self.stamp {
            return false;
        }
        self.stamp = stamp;
        match T::read(path) {
            Ok(held) => {
                self.held = held;
                self.unreadable = None;
            }
            Err(error) => {
                let file = path.display();
                self.unreadable = Some(format!("{file}: {error}. Nothing is written until that is put right."));
            }
        }
        true
    }

    /// Changes what is held, and writes what changed at once if `change` says that it did
    /// change something: then this is true. What is changed is what the files have now:
    /// anything that something else wrote there meanwhile is read first, and so is not lost.
    pub fn edit(&mut self, change: impl FnOnce(&mut T) -> bool) -> bool {
        self.read_again();
        if !change(&mut self.held) {
            return false;
        }
        if let Some(path) = self.path.as_ref().filter(|_| self.unreadable.is_none()) {
            let failed = |error: io::Error| format!("{} could not be written: {error}.", path.display());
            self.unwritten = self.held.write(path).err().map(failed);
            // Something else may have written another file of the folder since it was read
            // a moment ago: with no stamp the next look reads everything again, which is
            // cheap, rather than take the folder as it is now for what is held.
            self.stamp = None;
        }
        true
    }

    /// Whether the file is one that can be read, or is not there at all.
    pub fn readable(&self) -> bool {
        self.unreadable.is_none()
    }

    /// What is wrong with the file, for as long as it is: it cannot be read, or it could not
    /// be written.
    pub fn trouble(&self) -> Option<&str> {
        self.unreadable.as_deref().or(self.unwritten.as_deref())
    }
}
