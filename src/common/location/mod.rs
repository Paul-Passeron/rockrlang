use std::{fmt::Display, path::PathBuf, sync::Arc};

use crate::{Db, get_source_file, ril::ModuleId};

#[salsa::interned]
pub struct SourceFile {
    pub path: PathBuf,
    pub content: Arc<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OwnedSourceFile {
    pub path: PathBuf,
    pub content: Arc<String>,
}

impl OwnedSourceFile {
    pub fn new(path: PathBuf, content: Arc<String>) -> Self {
        Self { path, content }
    }

    pub fn to_source_file<'db>(&self, db: &'db dyn Db) -> SourceFile<'db> {
        SourceFile::new(db, self.path.clone(), self.content.clone())
    }
}

impl<'db> SourceFile<'db> {
    pub fn to_owned(&self, db: &'db dyn Db) -> OwnedSourceFile {
        OwnedSourceFile::new(self.path(db), self.content(db))
    }
}

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

impl Location {
    pub fn loc_info(&self, db: &dyn crate::Db, module: ModuleId) -> Option<LocationInfo> {
        let package = module.owning_package(db)?;
        let sf = get_source_file(db, self.file.as_path(), &[package])?;
        Some(get_loc_info(db, sf, self.offset))
    }
}

#[salsa::tracked]
pub fn get_loc_info<'db>(
    db: &'db dyn crate::Db,
    file: SourceFile<'db>,
    offset: usize,
) -> LocationInfo {
    let s = file.content(db);
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
        let cwd = std::env::current_dir().unwrap_or_default();
        let path = pathdiff::diff_paths(&self.file, cwd).unwrap_or(self.file.clone());
        write!(f, "{}:{}:{}", path.display(), self.line, self.column)
    }
}
