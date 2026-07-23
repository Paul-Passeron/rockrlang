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

use crate::{
    Db, SourceFile,
    driver::ANCHOR_FILE_NAME,
    resolved::{FileModule, Package},
};
use dashmap::DashSet;
use itertools::Itertools;
use salsa::Setter;
use std::{fmt, path::PathBuf, sync::Arc};

#[salsa::input(singleton)]
pub struct Workspace {
    pub config: Config,
    pub files: DashSet<SourceFile>,
    pub roots: DashSet<PackageRoot>,
}

#[salsa::input]
pub struct PackageRoot {
    pub name: String,
    pub file: SourceFile,
}

impl Workspace {
    pub fn initialize(db: &dyn Db, config: Config) -> Self {
        Self::new(db, config, DashSet::new(), DashSet::new())
    }

    pub fn add_file(self, db: &mut dyn Db, file: SourceFile) {
        let files = self.files(db).clone();
        if files.insert(file) {
            self.set_files(db).to(files);
        }
    }

    pub fn remove_file(self, db: &mut dyn Db, file: SourceFile) {
        let files = self.files(db).clone();
        if files.remove(&file).is_some() {
            self.set_files(db).to(files);
        }
    }

    pub fn add_package_root(self, db: &mut dyn Db, root: PackageRoot) {
        let roots = self.roots(db).clone();
        let new_name = root.name(db);
        let new_file = root.file(db);
        let ws = Workspace::get(db);
        for known in roots.iter() {
            if known.name(db) == new_name {
                if known.file(db) == new_file {
                    return;
                }
                // This should not happen, so we'll see what to do in this case
                return;
            }
        }
        roots.insert(root);
        ws.set_roots(db).to(roots);
    }

    pub fn to_string(self, db: &dyn Db) -> String {
        format!(
            "Workspace {{\n    config: {:?},\n    files:\n        {}\n}}",
            self.config(db),
            self.files(db)
                .iter()
                .sorted_by_key(|x| x.path(db))
                .map(|f| f.path(db).display().to_string())
                .join("\n        ")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub no_std: bool,
    pub skip_core: bool,
    pub display_llvm: bool,
    pub display_opt_llvm: bool,
    pub display_mir: bool,
    pub display_thir: bool,
    pub compile_only: bool,
    pub output: Option<PathBuf>,
}

pub enum CompilerError {
    NoCompilationUnitFound(PathBuf),
    STDLibNotFound,
    NoFileFoundAt(PathBuf),
    CoreLibNotFound,
    CompiledWithErrors,
    LinkFailed(String),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompilerError::NoCompilationUnitFound(path_buf) => {
                write!(f, "No compilation unit found at `{}`", path_buf.display())
            }
            CompilerError::STDLibNotFound => {
                write!(f, "Standard library (`std`) package not found.")
            }
            CompilerError::NoFileFoundAt(path_buf) => {
                write!(f, "No file found at {}", path_buf.display())
            }
            CompilerError::CoreLibNotFound => {
                write!(f, "Core library (`core`) package not found.")
            }
            CompilerError::CompiledWithErrors => {
                write!(f, "Errors encountered, did not compile.")
            }
            CompilerError::LinkFailed(msg) => {
                write!(f, "Linking failed: {msg}")
            }
        }
    }
}

#[salsa::tracked(returns(copy))]
pub fn is_file_direct_submodule_of_file(
    db: &dyn Db,
    parent: SourceFile,
    child: SourceFile,
) -> bool {
    if parent == child {
        return false;
    }
    let p_path = parent.path(db);
    if p_path.file_name().unwrap() != ANCHOR_FILE_NAME {
        return false;
    }
    let c_path = child.path(db);
    let parent_dir = p_path.parent().unwrap();
    let child_dir = c_path.parent().unwrap();

    if parent_dir == child_dir {
        return true;
    }

    if c_path.file_name().unwrap() == ANCHOR_FILE_NAME
        && child_dir.parent() == Some(parent_dir)
    {
        return true;
    }

    false
}

#[salsa::tracked]
pub fn submodules_of_file<'db>(
    db: &'db dyn Db,
    file: SourceFile,
) -> Vec<FileModule<'db>> {
    if file.path(db).file_name().unwrap() != ANCHOR_FILE_NAME {
        return vec![];
    }

    let ws = Workspace::get(db);

    ws.files(db)
        .iter()
        .map(|sf| *sf)
        .filter(|sf| is_file_direct_submodule_of_file(db, file, *sf))
        .map(|sf| FileModule::new(db, sf, submodules_of_file(db, sf).to_vec()))
        .collect()
}

#[salsa::tracked]
pub fn package_of_root<'db>(db: &'db dyn Db, root: PackageRoot) -> Package<'db> {
    let file = *root.file(db);
    Package::new(db, FileModule::new(db, file, submodules_of_file(db, file).to_vec()))
}

#[salsa::tracked]
pub fn workspace_packages<'db>(db: &'db dyn Db, ws: Workspace) -> Arc<Vec<Package<'db>>> {
    Arc::new(ws.roots(db).iter().map(|root| *package_of_root(db, *root)).collect())
}
