/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use crate::Db;
use std::{fmt::Display, path::PathBuf, sync::Arc};

#[salsa::input]
#[derive(Debug, PartialOrd, Ord)]
pub struct SourceFile {
    #[returns(ref)]
    pub path: PathBuf,
    #[returns(ref)]
    pub content: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: SourceFile,
    pub offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub file: SourceFile,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl Span {
    pub fn start(self) -> Location {
        Location {
            file: self.file,
            offset: self.start_offset,
        }
    }

    pub fn end(self) -> Location {
        Location {
            file: self.file,
            offset: self.end_offset,
        }
    }

    pub fn new(
        file: SourceFile,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        Self {
            file,
            start_offset,
            end_offset,
        }
    }
}

impl Location {
    pub fn advance(self, offset: usize) -> Self {
        // TODO: maybe verify the validity
        let mut this = self;
        this.offset += offset;
        this
    }

    pub fn span(self, other: Self) -> Span {
        let file = self.file;
        if file != other.file {
            panic!("Span across different files");
        }
        let (start_offset, end_offset) = if self.offset > other.offset {
            (other.offset, self.offset)
        } else {
            (self.offset, other.offset)
        };
        Span {
            file,
            start_offset,
            end_offset,
        }
    }

    pub fn loc_info(self, db: &dyn Db) -> LocationInfo {
        _loc_info(db, self)
    }

    pub fn new(file: SourceFile, offset: usize) -> Self {
        Self { file, offset }
    }
}

fn _loc_info<'db>(db: &'db dyn Db, loc: Location) -> LocationInfo {
    #[salsa::interned]
    struct Interned {
        inner: Location,
    }
    #[salsa::tracked]
    fn _tracked<'a>(db: &'a dyn Db, loc: Interned<'a>) -> LocationInfo {
        let loc = loc.inner(db);
        let mut line = 1;
        let mut column = 1;
        let offset = loc.offset;
        let contents = &loc.file.content(db)[..offset];
        for c in contents.chars() {
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        LocationInfo {
            file: loc.file.path(db).clone(),
            line,
            column,
            offset,
        }
    }
    _tracked(db, Interned::new(db, loc))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocationInfo {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

impl PartialOrd for LocationInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LocationInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.file
            .cmp(&other.file)
            .then_with(|| self.offset.cmp(&other.offset))
    }
}

impl Display for LocationInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cwd = std::env::current_dir().unwrap_or_default();
        let path =
            pathdiff::diff_paths(&self.file, cwd).unwrap_or(self.file.clone());
        write!(f, "{}:{}:{}", path.display(), self.line, self.column)
    }
}
