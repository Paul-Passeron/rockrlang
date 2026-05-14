use clap::Parser;
use clap_derive::Parser;
use salsa::Database as Db;
use std::{path::PathBuf, sync::Arc};

use crate::{
    common::location::get_loc_info,
    driver::parsed_args_from_cli,
    parser::{ParseError, parse_file},
};

mod common;
mod driver;
mod lexer;
mod parse_tree;
mod parser;
mod tests;

#[derive(Debug, Parser)]
pub struct CliArgs {
    files: Option<Vec<PathBuf>>,
}

#[salsa::db]
#[derive(Clone, Default)]
pub struct RockrDb {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for RockrDb {}

#[salsa::input]
pub struct SourceFileContent {
    pub file: PathBuf,
    pub content: Arc<String>,
}

#[salsa::input]
pub struct SourceRoot {
    pub files: Vec<SourceFileContent>,
}

#[salsa::interned]
pub struct SourceFile {
    pub path: PathBuf,
}

#[salsa::tracked]
pub fn lookup_file<'db>(
    db: &'db dyn crate::Db,
    root: SourceRoot,
    file: SourceFile<'db>,
) -> Option<SourceFileContent> {
    root.files(db)
        .iter()
        .copied()
        .find(|f| f.file(db) == file.path(db))
}

fn main() -> Result<(), String> {
    let db = RockrDb::default();
    let args = CliArgs::parse();
    let parsed = parsed_args_from_cli(&db, &args).ok_or("No input file provided".to_string())?;
    let root = SourceRoot::new(&db, parsed.files.into());
    let mut has_errors = false;
    for file in root.files(&db) {
        println!("-----------------------------------------------------");
        println!("File: {}", file.file(&db).display());
        println!("-----------------------------------------------------");
        let source = SourceFile::new(&db, file.file(&db));
        let parsed = parse_file(&db, root, source);
        let errors: Vec<&ParseError> = parse_file::accumulated::<ParseError>(&db, root, source);
        for error in &errors {
            let info = get_loc_info(&db, root, source, error.start);
            eprintln!("{info}: {:?}", error.kind);
            has_errors = true;
        }

        for item in parsed.items(&db) {
            println!("{item:#?}");
        }
    }
    if has_errors {
        Err("Compiled with some errors".to_string())
    } else {
        Ok(())
    }
}
