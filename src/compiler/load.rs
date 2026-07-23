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
    Db, RockrDb,
    compiler::{CompilerError, Config, PackageRoot, Workspace},
    driver::{ANCHOR_FILE_NAME, read_source_file},
};
use std::path::PathBuf;
use walkdir::WalkDir;

fn add_package_root_from_disk(
    db: &mut dyn Db,
    root: PathBuf,
) -> Result<(), CompilerError> {
    let root = root.canonicalize().map_err(|_| CompilerError::NoFileFoundAt(root))?;
    let path_to_file =
        if root.is_dir() { root.join(ANCHOR_FILE_NAME) } else { root.clone() };
    let package_name = root.file_name().unwrap().to_str().unwrap().to_string(); // Should not fail on well-formed canonicalized paths
    let root_file =
        read_source_file(db, &path_to_file).ok_or(CompilerError::NoFileFoundAt(root))?;
    let root = PackageRoot::new(db, package_name, root_file);
    Workspace::get(db).add_package_root(db, root);
    Ok(())
}

fn core_path() -> Result<PathBuf, CompilerError> {
    path_from_env("ROCKR_CORE")
}

fn std_path() -> Result<PathBuf, CompilerError> {
    path_from_env("ROCKR_STD")
}

fn path_from_env(env: &str) -> Result<PathBuf, CompilerError> {
    std::env::var(env).map_err(|_| CompilerError::CoreLibNotFound).and_then(|path| {
        PathBuf::from(path).canonicalize().map_err(|_| CompilerError::CoreLibNotFound)
    })
}

pub fn compute_package_roots(
    db: &mut dyn Db,
    root: PathBuf,
) -> Result<(), CompilerError> {
    add_package_root_from_disk(db, root)?;
    add_package_root_from_disk(db, core_path()?)?;
    if !db.config().no_std {
        add_package_root_from_disk(db, std_path()?)?;
    }
    Ok(())
}

pub fn compute_all_files_from_roots(db: &mut dyn Db) -> Result<(), CompilerError> {
    fn walk(db: &mut dyn Db, p: PathBuf) -> Result<(), CompilerError> {
        if p.is_dir() {
            WalkDir::new(p.clone())
                .into_iter()
                .filter_map(Result::ok)
                .filter(|path| {
                    if path.path() == p {
                        return false;
                    }
                    if path.file_type().is_file() {
                        path.path().extension().is_some_and(|ext| ext == "rkr")
                    } else {
                        true
                    }
                })
                .map(|e| e.into_path())
                .try_for_each(|p| walk(db, p))?;
        } else if db.find_source_file(&p).is_none() {
            read_source_file(db, &p)
                .ok_or_else(|| CompilerError::NoFileFoundAt(p.clone()))?;
        }

        Ok(())
    }
    let ws = Workspace::get(db);
    for root in ws.roots(db).clone().iter() {
        let path = root.file(db).path(db).clone();
        walk(
            db,
            if path.is_file()
                && path.file_name().unwrap().to_str().unwrap() == ANCHOR_FILE_NAME
            {
                path.parent().unwrap().to_path_buf()
            } else {
                path
            },
        )?;
    }
    Ok(())
}

pub fn load_workspace_from_disk(
    root: PathBuf,
    config: Config,
) -> Result<RockrDb, CompilerError> {
    let mut db = RockrDb::new();
    Workspace::initialize(&db, config);
    compute_package_roots(&mut db, root)?;
    compute_all_files_from_roots(&mut db)?;
    Ok(db)
}
