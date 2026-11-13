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

use walkdir::WalkDir;

use crate::{
    Db, RockrDb,
    check::check,
    common::{diagnostic::Diag, location::LocationInfo},
    printer::render_diagnostics,
    workspace::{
        PackageRoot, Workspace,
        config::Config,
        discover::{ANCHOR_FILE_NAME, read_source_file},
        error::CompilerError,
    },
};
use std::path::PathBuf;

fn add_package_root_from_disk(
    db: &mut dyn Db,
    root: PathBuf,
) -> Result<(), CompilerError> {
    let root = root.canonicalize().map_err(|_| CompilerError::NoFileFoundAt(root))?;
    let path_to_file =
        if root.is_dir() { root.join(ANCHOR_FILE_NAME) } else { root.clone() };
    let package_name = root.file_name().unwrap().to_str().unwrap().to_owned(); // Should not fail on well-formed canonicalized paths
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

fn enclosing_package_root(path: PathBuf) -> PathBuf {
    let Ok(path) = path.canonicalize() else {
        return path;
    };
    let Some(mut dir) = path.is_dir().then_some(path.as_path()).or_else(|| path.parent())
    else {
        return path;
    };
    if !dir.join(ANCHOR_FILE_NAME).is_file() {
        return path;
    }
    while let Some(parent) = dir.parent()
        && parent.join(ANCHOR_FILE_NAME).is_file()
    {
        dir = parent;
    }
    dir.to_path_buf()
}

pub fn compute_package_roots(
    db: &mut dyn Db,
    root: PathBuf,
) -> Result<(), CompilerError> {
    add_package_root_from_disk(db, enclosing_package_root(root))?;
    add_package_root_from_disk(db, core_path()?)?;
    if !db.config().no_std {
        add_package_root_from_disk(db, std_path()?)?;
    }
    Ok(())
}

pub fn compute_all_files_from_roots(db: &mut dyn Db) -> Result<(), CompilerError> {
    let ws = Workspace::get(db);
    for root in ws.roots(db).clone().iter() {
        let path = root.file(db).path(db).clone();
        let dir = if path.file_name().is_some_and(|n| n == ANCHOR_FILE_NAME) {
            path.parent().unwrap().to_path_buf()
        } else {
            path
        };
        let files = WalkDir::new(&dir)
            .sort_by_file_name()
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .filter(|p| p == &dir || p.extension().is_some_and(|ext| ext == "rkr"));
        for file in files {
            if db.find_source_file(&file).is_none() {
                read_source_file(db, &file)
                    .ok_or_else(|| CompilerError::NoFileFoundAt(file.clone()))?;
            }
        }
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

pub fn program_has_errors(db: &dyn Db) -> (bool, Vec<&Diag>) {
    let ws = Workspace::get(db);
    let raw_diags: Vec<&Diag> = check::accumulated::<Diag>(db, ws);
    let has_errors = raw_diags.iter().any(|d| d.is_error());
    (has_errors, raw_diags)
}

pub fn check_from_disk(root: PathBuf, config: Config) -> Result<(), CompilerError> {
    let db = load_workspace_from_disk(root, config)?;
    let ws = Workspace::get(&db);
    check(&db, ws);
    let (has_errors, raw_diags) = program_has_errors(&db);

    let mut diags: Vec<(&LocationInfo, &Diag)> = raw_diags
        .into_iter()
        .map(|d| (d.primary.span.start().loc_info(&db), d))
        .collect();

    diags.sort_by(|(loc_a, diag_a), (loc_b, diag_b)| {
        diag_a.severity.cmp(&diag_b.severity).then_with(|| {
            loc_a.cmp(loc_b).then_with(|| diag_a.message.cmp(&diag_b.message))
        })
    });

    render_diagnostics(&db, diags.into_iter().map(|(_, diag)| diag));
    if has_errors { Err(CompilerError::CompiledWithErrors) } else { Ok(()) }
}
