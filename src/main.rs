use clap::Parser;
use clap_derive::Parser;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    common::location::get_loc_info,
    driver::load_package,
    hir::{FunctionLikeAst, function_ast, hir_body},
    name_resolve::{
        core_package,
        definition::{Definition, module_definitions},
        file_module_id,
        implems::module_impls,
        std_package,
    },
    parse_tree::top_level::AstImplItem,
    parser::{ParseError, parse_file},
    ril::{FileModule, FunctionId, ModuleId, Package, ScopeOwnerId, display::RilDisplay},
    thir::type_check_function,
};

mod common;
mod driver;
mod hir;
mod lexer;
mod name_resolve;
mod parse_tree;
mod parser;
mod ril;
mod thir;

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

fn get_source_file_in_submodule<'db>(
    db: &'db dyn Db,
    file: impl AsRef<Path>,
    submodule: FileModule<'db>,
) -> Option<SourceFile<'db>> {
    if submodule.file(db).path(db).as_path() == file.as_ref() {
        return Some(submodule.file(db));
    }
    for submodule in submodule.submodules(db) {
        if let Some(res) = get_source_file_in_submodule(db, file.as_ref(), *submodule) {
            return Some(res);
        }
    }
    None
}

fn get_source_file<'db>(
    db: &'db dyn Db,
    file: impl AsRef<Path>,
    packages: &[Package<'db>],
) -> Option<SourceFile<'db>> {
    for package in packages {
        let source_file = package.root(db).file(db);
        if source_file.path(db) == file.as_ref() {
            return Some(source_file);
        }
    }
    for package in packages {
        for submodule in package.root(db).submodules(db) {
            if let Some(res) = get_source_file_in_submodule(db, file.as_ref(), *submodule) {
                return Some(res);
            }
        }
    }
    None
}

fn check_module<'db>(db: &'db dyn Db, module: ModuleId, packages: Vec<Package<'db>>) -> bool {
    let defs = module_definitions(db, module.interned());
    let mut v = defs.values().copied().collect::<Vec<_>>();
    v.sort();
    for def in v {
        match def {
            Definition::Function(function_id) => {
                println!("---------------------------------");
                println!("{}", function_id.display(db));
                println!("---------------------------------");
                if let Some(hir) = hir_body(db, function_id.interned()) {
                    println!("{}", hir.display(db));
                }
                if let Some(results) = type_check_function(
                    db,
                    function_id.interned(),
                    packages.clone().into_boxed_slice(),
                ) {
                    for (expr_id, ty) in &results.node_types(db) {
                        println!("{:?}: {}", expr_id, ty.display(db));
                    }
                    for diagnostic in &results.diagnostics(db) {
                        let loc = &diagnostic.span;
                        let source_file = get_source_file(db, &loc.file, &packages).unwrap();
                        let loc_info = get_loc_info(db, source_file, loc.start);
                        println!("{}: {:?}", loc_info, diagnostic.kind);
                    }
                } else if let FunctionLikeAst::Fundef(_) =
                    function_ast(db, function_id.interned()).inner(db)
                {
                    panic!("No type check results !")
                }
            }
            _ => (),
        }
    }

    for implem in module_impls(db, module.interned()) {
        for item in implem.items(db) {
            match item {
                AstImplItem::Type { .. } => (),
                AstImplItem::Fundef(spanned) => {
                    let id =
                        FunctionId::new(db, spanned.data.name, ScopeOwnerId::Impl(implem.id(db)));
                    if let Some(hir) = hir_body(db, id.interned()) {
                        println!("{}", hir.display(db));
                    }
                }
            }
        }
    }

    false
}

fn check_file<'db>(
    db: &'db dyn Db,
    source: SourceFile<'db>,
    module_id: ModuleId,
    packages: Vec<Package<'db>>,
) -> bool {
    let mut has_errors = false;
    let errors: Vec<&ParseError> = parse_file::accumulated::<ParseError>(db, source);
    for error in &errors {
        let info = get_loc_info(db, source, error.start);
        eprintln!("{info}: {:?}", error.kind);
        has_errors = true;
    }
    has_errors |= check_module(db, module_id, packages);
    has_errors
}

fn check_module_tree<'db>(
    db: &'db dyn Db,
    file_module: FileModule<'db>,
    parent: Option<ModuleId>,
    package: Package<'db>,
    packages: Vec<Package<'db>>,
) -> bool {
    let module_id = file_module_id(db, file_module, parent, package);
    let mut has_errors = check_file(db, file_module.file(db), module_id, packages.clone());
    for sub in file_module.submodules(db) {
        has_errors |= check_module_tree(db, *sub, Some(module_id), package, packages.clone());
    }
    has_errors
}

fn try_package<'db>(db: &'db dyn Db, package: Package<'db>, packages: Vec<Package<'db>>) -> bool {
    check_module_tree(db, package.root(db), None, package, packages)
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

    let mut packages = vec![package, core_package(&db)];
    if !db.config.no_std {
        packages.push(std_package(&db).unwrap());
    }

    println!("Packages structure:");
    for package in &packages {
        print_module_tree(&db, package.root(&db), 0);
    }
    println!();

    let mut has_errors = false;
    let cloned = packages.clone();
    for package in &packages {
        has_errors |= try_package(&db, *package, cloned.clone());
    }

    if has_errors {
        Err("Compiled with some errors".to_string())
    } else {
        Ok(())
    }
}
