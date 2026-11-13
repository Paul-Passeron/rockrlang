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

use crate::{Db, SourceFile, workspace::config::Config};
use dashmap::DashSet;
use itertools::Itertools;
use salsa::Setter;

pub mod config;
pub mod discover;
pub mod error;
pub mod load;

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
        let ws = Self::get(db);
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
