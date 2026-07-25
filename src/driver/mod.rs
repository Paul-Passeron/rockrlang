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

use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use crate::{Db, SourceFile};

pub const ANCHOR_FILE_NAME: &str = "main.rkr";

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DiscoveredModule {
    pub path: PathBuf,
    pub submodules: Vec<Self>,
}

pub fn discover_package(p: &Path) -> Option<DiscoveredModule> {
    // let p = p.canonicalize().ok()?;
    if p.is_dir() {
        discover_dir(p)
    } else if p.file_name()? == ANCHOR_FILE_NAME {
        discover_dir(p.parent()?)
    } else {
        // Standalone file — no submodules
        Some(DiscoveredModule { path: p.to_path_buf(), submodules: vec![] })
    }
}

fn discover_dir(dir: &Path) -> Option<DiscoveredModule> {
    let main = dir.join(ANCHOR_FILE_NAME);
    if !main.exists() {
        return None;
    }

    let mut submodules = vec![];
    for entry in fs::read_dir(dir).ok()? {
        let entry_path = entry.ok()?.path();
        if entry_path == main {
            continue; // skip main.rkr itself — it's the root, not a submodule
        }
        if entry_path.is_dir() {
            // Only include directories that themselves have a main.rkr
            if let Some(m) = discover_dir(&entry_path) {
                submodules.push(m);
            }
        } else if entry_path.extension().and_then(|e| e.to_str()) == Some("rkr") {
            submodules.push(DiscoveredModule { path: entry_path, submodules: vec![] });
        }
    }
    Some(DiscoveredModule { path: main, submodules })
}

#[salsa::interned]
pub struct SalsaPath<'db> {
    pub value: PathBuf,
}

pub fn read_source_file(db: &mut dyn Db, path: &Path) -> Option<SourceFile> {
    let mut s = String::new();
    File::open(path).ok()?.read_to_string(&mut s).ok()?;
    db.add_source_file(path.to_path_buf(), s).ok()
}
