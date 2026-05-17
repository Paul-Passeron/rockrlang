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
    sync::Arc,
};

use crate::{
    Db, SourceFile,
    ril::{FileModule, Package},
};

const ANCHOR_FILE_NAME: &str = "main.rkr";

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DiscoveredModule {
    pub path: PathBuf,
    pub submodules: Vec<DiscoveredModule>,
}

pub fn discover_package(p: &Path) -> Option<DiscoveredModule> {
    // let p = p.canonicalize().ok()?;
    if p.is_dir() {
        discover_dir(&p)
    } else if p.file_name()? == ANCHOR_FILE_NAME {
        discover_dir(p.parent()?)
    } else {
        // Standalone file — no submodules
        Some(DiscoveredModule {
            path: p.to_path_buf(),
            submodules: vec![],
        })
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
            submodules.push(DiscoveredModule {
                path: entry_path,
                submodules: vec![],
            });
        }
    }
    Some(DiscoveredModule {
        path: main,
        submodules,
    })
}

#[salsa::interned]
pub struct SalsaPath<'db> {
    #[returns(ref)]
    pub value: PathBuf,
}

fn read_source_file<'db>(db: &'db dyn Db, path: &Path) -> Option<SourceFile<'db>> {
    let mut s = String::new();
    File::open(path).ok()?.read_to_string(&mut s).ok()?;
    Some(SourceFile::new(db, path.to_path_buf(), Arc::new(s)))
}

#[salsa::tracked]
pub fn load_file_module<'db>(db: &'db dyn Db, path: SalsaPath<'db>) -> Option<FileModule<'db>> {
    let p = path.value(db);
    let file = read_source_file(db, p)?;

    let submodule_paths = discover_direct_children(p);
    let submodules = submodule_paths
        .into_iter()
        .filter_map(|child| {
            let salsa_path = SalsaPath::new(db, child);
            load_file_module(db, salsa_path)
        })
        .collect::<Vec<_>>();

    Some(FileModule::new(db, file, submodules))
}

fn discover_direct_children(path: &Path) -> Vec<PathBuf> {
    let is_anchor = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == ANCHOR_FILE_NAME)
        .unwrap_or(false);

    if !is_anchor {
        return vec![];
    }

    let dir = match path.parent() {
        Some(d) => d,
        None => return vec![],
    };

    let mut children = vec![];
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path == path {
            continue; // skip main.rkr itself
        }
        if entry_path.is_dir() {
            let nested_main = entry_path.join(ANCHOR_FILE_NAME);
            if nested_main.exists() {
                children.push(nested_main);
            }
        } else if entry_path.extension().and_then(|e| e.to_str()) == Some("rkr") {
            children.push(entry_path);
        }
    }
    children
}

pub fn load_package<'db>(db: &'db dyn Db, p: &Path) -> Option<Package<'db>> {
    let discovered = discover_package(p)?;
    let salsa_path = SalsaPath::new(db, discovered.path);
    load_package_tracked(db, salsa_path)
}

#[salsa::tracked]
fn load_package_tracked<'db>(db: &'db dyn Db, path: SalsaPath<'db>) -> Option<Package<'db>> {
    let root = load_file_module(db, path)?;
    Some(Package::new(db, root))
}
