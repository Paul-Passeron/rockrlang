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

use crate::model::{FileModule, Package};
use rockr_base::{
    Db, SourceFile,
    workspace::{PackageRoot, Workspace, discover::ANCHOR_FILE_NAME},
};

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

#[salsa::tracked(returns(deref))]
pub fn submodules_of_file(db: &dyn Db, file: SourceFile) -> Vec<FileModule<'_>> {
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
pub fn package_of_root(db: &dyn Db, root: PackageRoot) -> Package<'_> {
    let file = *root.file(db);
    Package::new(db, FileModule::new(db, file, submodules_of_file(db, file).to_vec()))
}

#[salsa::tracked(returns(deref))]
pub fn workspace_packages(db: &dyn Db, ws: Workspace) -> Vec<Package<'_>> {
    ws.roots(db).iter().map(|root| *package_of_root(db, *root)).collect()
}
