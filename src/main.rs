use clap::Parser;
use clap_derive::Parser;
use salsa::Database as Db;
use std::{path::PathBuf, sync::Arc};

use crate::{
    common::location::get_loc_info,
    driver::load_package,
    name_resolve::{
        definition::{get_module_pretty_name, module_definitions, resolve_type_expr},
        file_module_id, module_items,
    },
    parse_tree::top_level::AstTopLevelItemDesc,
    parser::{ParseError, parse_file},
    ril::{FileModule, ModuleId},
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
    file: Option<PathBuf>,
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
    pub fn to_owned(&self, db: &'db dyn Db) -> OwnedSourceFile {
        OwnedSourceFile::new(self.path(db), self.content(db))
    }
}

fn print_module_tree<'db>(db: &'db RockrDb, module: FileModule<'db>, indent: usize) {
    let prefix = "  ".repeat(indent);
    println!(
        "{}[{}] {}",
        prefix,
        module.name(db).interned().contents(db),
        module.file(db).path(db).display()
    );
    for sub in module.submodules(db) {
        print_module_tree(db, *sub, indent + 1);
    }
}

fn check_module<'db>(db: &'db dyn Db, module: ModuleId) -> bool {
    let defs = module_definitions(db, module.interned());
    println!("***************************************************");
    println!(
        "Checking module {}:",
        get_module_pretty_name(db, module.interned())
    );

    println!("Module definitions:");
    for (name, def) in &defs {
        println!("  {} => {:?}", name.interned().contents(db), def);
    }

    let Some(items) = module_items(db, module.interned()) else {
        println!("  (file-backed module — items not yet wired)");
        println!("------------------------------------------------");
        return false;
    };

    println!("Function signatures:");
    let mut has_errors = false;

    for item in items {
        if let AstTopLevelItemDesc::Fundef(fundef) = &item.data {
            let fd = &fundef.data;
            println!("  fn {}:", fd.name.interned().contents(db));
            for arg in &fd.args {
                let resolved = resolve_type_expr(db, &arg.ty, module.interned(), &fd.template_args);
                println!("    {} : {:?}", arg.name.interned().contents(db), resolved);
            }
            let ret = resolve_type_expr(db, &fd.return_type, module.interned(), &fd.template_args);
            println!("    -> {:?}", ret);
        } else if let AstTopLevelItemDesc::Module(module_ast) = &item.data {
            let child_id = ModuleId::new(db, module_ast.data.name, Some(module), None, vec![]);
            has_errors |= check_module(db, child_id);
        }
    }
    println!("***************************************************");

    has_errors
}

fn check_file<'db>(db: &'db RockrDb, source: SourceFile<'db>, module_id: ModuleId) -> bool {
    let mut has_errors = false;
    let errors: Vec<&ParseError> = parse_file::accumulated::<ParseError>(db, source);
    for error in &errors {
        let info = get_loc_info(db, source, error.start);
        eprintln!("{info}: {:?}", error.kind);
        has_errors = true;
    }
    has_errors |= check_module(db, module_id);
    has_errors
}

fn check_module_tree<'db>(
    db: &'db RockrDb,
    file_module: FileModule<'db>,
    parent: Option<ModuleId>,
) -> bool {
    let module_id = file_module_id(db, file_module, parent);
    let mut has_errors = check_file(db, file_module.file(db), module_id);
    for sub in file_module.submodules(db) {
        has_errors |= check_module_tree(db, *sub, Some(module_id));
    }
    has_errors
}

fn main() -> Result<(), String> {
    let db = RockrDb::default();
    let args = CliArgs::parse();

    let current_dir = std::env::current_dir().unwrap();
    let root_path = args.file.as_deref().unwrap_or(&current_dir);

    let package = load_package(&db, root_path)
        .ok_or_else(|| format!("No package found at `{}`", root_path.display()))?;

    println!("Package structure:");
    print_module_tree(&db, package.root(&db), 0);
    println!();

    let has_errors = check_module_tree(&db, package.root(&db), None);

    if has_errors {
        Err("Compiled with some errors".to_string())
    } else {
        Ok(())
    }
}
