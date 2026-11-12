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

use dashmap::DashMap;
use salsa::Setter;
use std::path::{Path, PathBuf};

use crate::{
    SourceFile,
    workspace::{Workspace, config::Config},
};

#[salsa::db]
#[derive(Default)]
pub struct RockrDb {
    pub storage: salsa::Storage<Self>,
    pub files: DashMap<PathBuf, SourceFile>,
}

#[salsa::db]
pub trait Db: salsa::Database {
    fn config(&self) -> &Config {
        Workspace::get(self).config(self)
    }

    fn get_ref_files(&self) -> &DashMap<PathBuf, SourceFile>;

    fn get_mut_ref_files(&mut self) -> &mut DashMap<PathBuf, SourceFile>;
}

#[salsa::db]
impl Db for RockrDb {
    fn get_ref_files(&self) -> &DashMap<PathBuf, SourceFile> {
        &self.files
    }

    fn get_mut_ref_files(&mut self) -> &mut DashMap<PathBuf, SourceFile> {
        &mut self.files
    }
}

impl RockrDb {
    pub fn new() -> Self {
        Self::default()
    }
}

#[salsa::db]
impl salsa::Database for RockrDb {}

impl dyn Db {
    pub fn open_workspace(&self, config: Config) -> Workspace {
        Workspace::initialize(self, config)
    }

    pub fn add_source_file(&mut self, path: PathBuf, text: String) -> Option<SourceFile> {
        let path = path.canonicalize().ok()?;
        if let Some(existing) = self.get_ref_files().get(&path) {
            // We do not change the contents here. If that's the intent use this
            // in cunjunction with set_source_file_text.
            return Some(*existing);
        }
        let sf = SourceFile::new(self, path.clone(), text.into());
        self.get_mut_ref_files().insert(path, sf);
        let ws = Workspace::get(self);
        let files = ws.files(self).clone();
        files.insert(sf);
        ws.set_files(self).to(files);
        Some(sf)
    }

    pub fn find_source_file(&self, path: &Path) -> Option<SourceFile> {
        let path = path.canonicalize().ok()?;
        self.get_ref_files().get(&path).map(|val| *val)
    }

    pub fn set_source_file_text(&mut self, file: SourceFile, text: String) {
        file.set_content(self).to(text.into());
    }

    pub fn remove_source_file(&mut self, file: SourceFile) {
        let path = file.path(self).clone();
        self.get_mut_ref_files().remove(&path);
        let ws = Workspace::get(self);
        let files = ws.files(self).clone();
        files.remove(&file);
        ws.set_files(self).to(files);
    }
}
