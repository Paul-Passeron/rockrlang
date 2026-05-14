use std::{fmt::Display, path::PathBuf};

use crate::{SourceFile, SourceRoot, lookup_file};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub offset: usize,
    pub file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub file: PathBuf,
}

impl Span {
    pub fn new(start: usize, end: usize, file: PathBuf) -> Self {
        Self { start, end, file }
    }

    pub fn start(&self) -> Location {
        Location::new(self.start, self.file.clone())
    }

    pub fn end(&self) -> Location {
        Location::new(self.end, self.file.clone())
    }
}

impl Location {
    pub fn new(offset: usize, file: PathBuf) -> Self {
        Self { offset, file }
    }
}

impl Location {
    #[allow(dead_code)]
    pub fn span(&self, other: &Self) -> Span {
        assert!(self.file == other.file);
        let start_offset = self.offset.min(other.offset);
        let end_offset = self.offset.max(other.offset);
        Span::new(start_offset, end_offset, self.file.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocationInfo {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

#[salsa::tracked]
pub fn get_loc_info<'db>(
    db: &'db dyn crate::Db,
    root: SourceRoot,
    file: SourceFile<'db>,
    offset: usize,
) -> LocationInfo {
    let contents = lookup_file(db, root, file).unwrap();
    let s = contents.content(db);
    let mut line = 1;
    let mut column = 1;
    for i in 0..offset {
        if let Some(c) = s[i..].chars().next() {
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        } else {
            break;
        }
    }
    LocationInfo {
        file: file.path(db),
        line,
        column,
        offset,
    }
}

impl Display for LocationInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file.display(), self.line, self.column)
    }
}
