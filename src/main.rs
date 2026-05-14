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
mod name_resolve;
mod parse_tree;
mod parser;
mod ril;

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

#[salsa::interned]
pub struct SourceFile {
    pub path: PathBuf,
    pub content: Arc<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OwnedSourceFile {
    pub path: PathBuf,
    pub content: Arc<String>,
}

impl OwnedSourceFile {
    pub fn new(path: PathBuf, content: Arc<String>) -> Self {
        Self { path, content }
    }

    pub fn to_source_file<'db>(&self, db: &'db dyn Db) -> SourceFile<'db> {
        SourceFile::new(db, self.path.clone(), self.content.clone())
    }
}

impl<'db> SourceFile<'db> {
    pub fn to_owned(&self, db: &'db dyn crate::Db) -> OwnedSourceFile {
        OwnedSourceFile::new(self.path(db), self.content(db))
    }
}

fn main() -> Result<(), String> {
    let db = RockrDb::default();
    let args = CliArgs::parse();
    let parsed = parsed_args_from_cli(&db, &args).ok_or("No input file provided".to_string())?;
    let mut has_errors = false;
    for file in parsed.files {
        println!("-----------------------------------------------------");
        println!("File: {}", file.path(&db).display());
        println!("-----------------------------------------------------");
        let source = SourceFile::new(&db, file.path(&db), file.content(&db));
        let parsed = parse_file(&db, source);
        let errors: Vec<&ParseError> = parse_file::accumulated::<ParseError>(&db, source);
        for error in &errors {
            let info = get_loc_info(&db, source, error.start);
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
