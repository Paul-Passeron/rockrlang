use std::{
    collections::HashSet,
    fs::{self, File, metadata},
    io::{self, Read},
    iter::once,
    path::{Path, PathBuf},
    sync::Arc,
};

use nonempty::NonEmpty;

use crate::SourceFileContent;

const ANCHOR_FILE_NAME: &str = "main.rkr";

fn list_files(path: &Path) -> io::Result<Vec<PathBuf>> {
    fn _list_files(vec: &mut Vec<PathBuf>, path: &Path) -> io::Result<()> {
        if metadata(path)?.is_dir() {
            let paths = fs::read_dir(path)?;
            for path_result in paths {
                let full_path = path_result?.path();
                if metadata(&full_path)?.is_dir() {
                    _list_files(vec, &full_path)?
                } else {
                    vec.push(full_path);
                }
            }
        }
        Ok(())
    }
    let mut vec = Vec::new();
    _list_files(&mut vec, path)?;
    Ok(vec)
}

fn source_file_contents_from_path<P: AsRef<Path>>(
    db: &dyn crate::Db,
    p: P,
) -> Option<SourceFileContent> {
    let path = p.as_ref();
    let buf = PathBuf::from(path);
    let s = {
        let mut s = String::new();
        let mut f = File::open(path).ok()?;
        f.read_to_string(&mut s).ok()?;
        s
    };
    Some(SourceFileContent::new(db, buf, Arc::new(s)))
}

fn discover_files_from_dir<P: AsRef<Path>>(p: P) -> HashSet<PathBuf> {
    let mut files = HashSet::new();
    // we must look for anchor file (main.ul)
    let mut main_path = p.as_ref().to_path_buf();
    main_path.push(ANCHOR_FILE_NAME);
    if main_path.exists() {
        if let Ok(entries) = list_files(p.as_ref()) {
            for entry in entries {
                files.insert(entry);
            }
        }
    }
    files
}

fn discover_files_from_path<P: AsRef<Path>>(p: P) -> Option<HashSet<PathBuf>> {
    let p = p.as_ref().canonicalize().ok()?;
    if !p.exists() {
        None
    } else if p.is_dir() {
        let res = discover_files_from_dir(p);
        (!res.is_empty()).then_some(res)
    } else if p.file_name().unwrap_or_default() == ANCHOR_FILE_NAME {
        let dir_path = p
            .parent()
            .unwrap_or_else(|| Path::new("./"))
            .canonicalize()
            .ok()?;
        let res = discover_files_from_dir(dir_path);
        (!res.is_empty()).then_some(res)
    } else {
        Some(HashSet::from_iter(once(p)))
    }
}

fn get_all_files<P: AsRef<Path>>(
    db: &dyn crate::Db,
    paths: &[P],
) -> Option<HashSet<SourceFileContent>> {
    let mut files = HashSet::new();
    for p in paths {
        if let Some(f) = discover_files_from_path(p) {
            files.extend(f);
        }
    }
    let res = files
        .into_iter()
        .map(|p| source_file_contents_from_path(db, p))
        .collect::<Option<HashSet<_>>>();
    if let Some(res) = &res
        && res.is_empty()
    {
        None
    } else {
        res
    }
}

pub(super) fn get_non_empty_files(
    db: &dyn crate::Db,
    ps: Option<&[PathBuf]>,
) -> Option<NonEmpty<SourceFileContent>> {
    let files = get_all_files(
        db,
        ps.unwrap_or(&[std::env::current_dir().unwrap_or_default()]),
    )?;

    NonEmpty::from_vec(files.into_iter().collect())
}

#[allow(dead_code)]
pub fn get_non_empty_files_from_paths(
    db: &dyn crate::Db,
    ps: &[PathBuf],
) -> Option<NonEmpty<SourceFileContent>> {
    let files = get_all_files(db, ps)?;
    NonEmpty::from_vec(files.into_iter().collect())
}
