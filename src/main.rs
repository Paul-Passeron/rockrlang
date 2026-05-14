use clap::Parser;
use clap_derive::Parser;
use std::{path::PathBuf, sync::Arc};

use crate::{
    common::location::get_loc_info,
    driver::load_package,
    name_resolve::{
        definition::{get_module_pretty_name, module_definitions},
        file_module_id,
        implems::module_impls,
        module_items, std_package,
        type_expr::resolve_type_expr,
    },
    parse_tree::top_level::AstTopLevelItemDesc,
    parser::{ParseError, parse_file},
    ril::{FileModule, ModuleId, Package, display::RilDisplay},
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
    #[clap(long, default_value_t = false)]
    no_std: bool,
}

#[derive(Clone)]
pub struct CompilerConfig {
    pub no_std: bool,
}

#[salsa::db]
#[derive(Clone)]
pub struct RockrDb {
    storage: salsa::Storage<Self>,
    config: CompilerConfig,
}

#[salsa::db]
pub trait Db: salsa::Database {
    fn config(&self) -> &CompilerConfig;
}

#[salsa::db]
impl Db for RockrDb {
    fn config(&self) -> &CompilerConfig {
        &self.config
    }
}

impl RockrDb {
    fn new(config: CompilerConfig) -> Self {
        Self {
            storage: salsa::Storage::default(),
            config,
        }
    }
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

fn print_module_tree<'db>(db: &'db dyn Db, module: FileModule<'db>, indent: usize) {
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
        println!("  {} => {}", name.interned().contents(db), def.display(db));
    }

    let Some(items) = module_items(db, module.interned()) else {
        println!("  (file-backed module — items not yet wired)");
        println!("------------------------------------------------");
        return false;
    };

    let impls = module_impls(db, module.interned());

    println!("Function signatures:");
    let mut has_errors = false;

    for item in items {
        if let AstTopLevelItemDesc::Fundef(fundef) = &item.data {
            let fd = &fundef.data;
            println!("  fn {}:", fd.name.interned().contents(db));
            for arg in &fd.args {
                let resolved = resolve_type_expr(db, &arg.ty, module.interned(), &fd.template_args);
                println!(
                    "    {} : {}",
                    arg.name.interned().contents(db),
                    resolved.display(db)
                );
            }
            let ret = resolve_type_expr(db, &fd.return_type, module.interned(), &fd.template_args);
            println!("    -> {}", ret.display(db));
        } else if let AstTopLevelItemDesc::Module(module_ast) = &item.data {
            let child_id = ModuleId::new(
                db,
                module_ast.data.name,
                Some(module),
                None,
                vec![],
                module.package(db),
            );
            has_errors |= check_module(db, child_id);
        }
    }
    println!("Implementations:");
    for impl_ in impls {
        println!("    {} ({:?})", impl_.display(db), impl_);
    }

    println!("***************************************************");

    has_errors
}

fn check_file<'db>(db: &'db dyn Db, source: SourceFile<'db>, module_id: ModuleId) -> bool {
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
    db: &'db dyn Db,
    file_module: FileModule<'db>,
    parent: Option<ModuleId>,
    package: Package<'db>,
) -> bool {
    let module_id = file_module_id(db, file_module, parent, package);
    let mut has_errors = check_file(db, file_module.file(db), module_id);
    for sub in file_module.submodules(db) {
        has_errors |= check_module_tree(db, *sub, Some(module_id), package);
    }
    has_errors
}

fn try_package<'db>(db: &'db dyn Db, package: Package<'db>) -> bool {
    println!("Package structure:");
    print_module_tree(db, package.root(db), 0);
    println!();
    let has_errors = check_module_tree(db, package.root(db), None, package);
    has_errors
}

fn main() -> Result<(), String> {
    let args = CliArgs::parse();
    let cfg = CompilerConfig {
        no_std: args.no_std,
    };
    let db = RockrDb::new(cfg);

    let current_dir = std::env::current_dir().unwrap();
    let root_path = args.file.as_deref().unwrap_or(&current_dir);

    let package = load_package(&db, root_path)
        .ok_or_else(|| format!("No package found at `{}`", root_path.display()))?;

    let mut has_errors = false;
    has_errors |= try_package(&db, package);
    if !db.config.no_std {
        has_errors |= try_package(&db, std_package(&db).unwrap());
    }

    if has_errors {
        Err("Compiled with some errors".to_string())
    } else {
        Ok(())
    }
}
