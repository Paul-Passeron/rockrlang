use std::path::Path;
use std::{fmt, path::PathBuf};

use itertools::Itertools;

use crate::compiler::diagnostic::Diagnostic;
use crate::hir::{function_ast, hir_body};
use crate::name_resolve::definition::{Definition, get_module_pretty_name, module_definitions};
use crate::name_resolve::implems::module_impls;
use crate::name_resolve::type_expr::resolve_type_expr;
use crate::name_resolve::{core_package, file_module_id, std_package};
use crate::parse_tree::top_level::{self, AstImplItem};
use crate::parser::{ParseError, parse_file};
use crate::ril::display::RilDisplay;
use crate::ril::{
    FileModule, FunctionId, ImplSource, InterfaceId, ModuleId, Package, ScopeOwnerId, TypeDefId,
};
use crate::thir::{ExprId, type_check_function};
use crate::{Db, OwnedSourceFile, RockrDb, driver};

pub mod diagnostic;

#[derive(Clone)]
pub struct Config {
    pub no_std: bool,
    pub skip_core: bool,
}

pub struct PackageInfo {
    pub root: FileModuleInfo,
}

pub type SourceFileInfo = OwnedSourceFile;

pub struct FileModuleInfo {
    pub file: SourceFileInfo,
    pub submodules: Box<[FileModuleInfo]>,
}

impl<'a> FileModuleInfo {
    fn from(db: &dyn Db, file_module: FileModule<'a>) -> Self {
        Self {
            file: file_module.file(db).to_owned(db),
            submodules: file_module
                .submodules(db)
                .iter()
                .map(|submodule| FileModuleInfo::from(db, *submodule))
                .collect(),
        }
    }

    pub fn name(&self) -> String {
        let path = self.file.path.as_path();
        path.parent()
            .and_then(|parent| {
                // If this file is main.rkr, the module name is the directory name
                if path.file_name().and_then(|n| n.to_str()) == Some("main.rkr") {
                    parent.file_name()
                } else {
                    None
                }
            })
            .or_else(|| path.file_stem())
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    }
}

impl<'a> PackageInfo {
    fn from(db: &dyn Db, package: Package<'a>) -> Self {
        Self {
            root: FileModuleInfo::from(db, package.root(db)),
        }
    }
}

pub struct Report {
    pub packages: Box<[PackageInfo]>,
    pub diagnostics: Vec<Diagnostic>,
    pub funcs: Vec<FunctionResult>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        false
    }

    pub fn new(packages: Box<[PackageInfo]>) -> Self {
        Self {
            packages,
            diagnostics: Default::default(),
            funcs: Default::default(),
        }
    }
}

pub enum CompilerError {
    NoCompilationUnitFound(PathBuf),
    STDLibNotFound,
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompilerError::NoCompilationUnitFound(path_buf) => {
                write!(f, "No compilation unit found at `{}`", path_buf.display())
            }
            CompilerError::STDLibNotFound => {
                write!(f, "Standard library (`std`) package not found.")
            }
        }
    }
}

fn build_package_set<'a>(
    db: &'a dyn Db,
    package: Package<'a>,
) -> Result<Box<[Package<'a>]>, CompilerError> {
    let mut packages = vec![package, core_package(db)];
    if !db.config().no_std {
        packages.push(std_package(db).ok_or(CompilerError::STDLibNotFound)?);
    }
    Ok(packages.into_boxed_slice())
}

pub fn check_type_def<'a>(
    _db: &'a dyn Db,
    _type_def_id: TypeDefId,
    _package: Package<'a>,
    _packages: &[Package<'a>],
    _report: &mut Report,
) {
    // Nothing to do, I think ?
}

pub fn check_interface<'a>(
    db: &'a dyn Db,
    interface_id: InterfaceId,
    package: Package<'a>,
    packages: &[Package<'a>],
    report: &mut Report,
) {
    // todo!()
}

pub struct FunctionResult {
    pub name: String,
    pub hir: String,
    pub typed_exprs: Vec<(ExprId, String)>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn get_pretty_owner<'a>(db: &'a dyn Db, owner: ScopeOwnerId) -> String {
    let res = match owner {
        ScopeOwnerId::Module(module_id) => {
            get_module_pretty_name(db, module_id.interned()).to_string()
        }
        ScopeOwnerId::Impl(impl_id) => {
            let parent = get_module_pretty_name(db, impl_id.parent(db).interned());
            format!(
                "{parent}::`impl {}{}`",
                match impl_id.interface(db) {
                    Some(interface_ref) => format!("{} for ", interface_ref.def(db).display(db)),
                    _ => "".into(),
                },
                impl_id.implemented(db).display(db)
            )
        }
        ScopeOwnerId::Interface(interface_ref) => {
            let parent = get_module_pretty_name(db, interface_ref.def(db).parent(db).interned());
            format!("{parent}::{}", interface_ref.def(db).display(db))
        }
    };

    if let Some(res) = res.strip_prefix("@builtin::") {
        return res.into();
    }
    res
}

pub fn get_pretty_function<'a>(db: &'a dyn Db, function_id: FunctionId) -> String {
    let ast = function_ast(db, function_id.interned()).inner(db);

    format!(
        "{}::{}({}{}): {}",
        get_pretty_owner(db, function_id.parent(db)),
        function_id.name(db).display(db),
        ast.receiver()
            .map_or(String::new(), |receiver| receiver.to_string()),
        ast.get_args()
            .iter()
            .map(|arg| arg.display(db).to_string())
            .collect_vec()
            .join(", "),
        ast.get_ret().data.display(db)
    )
}

pub fn check_function<'a>(
    db: &'a dyn Db,
    function_id: FunctionId,
    packages: &[Package<'a>],
    report: &mut Report,
) {
    let name = get_pretty_function(db, function_id);
    let thir = type_check_function(db, function_id.interned(), packages.into());
    report.funcs.push(FunctionResult {
        name,
        hir: hir_body(db, function_id.interned())
            .map_or(String::new(), |hir| hir.display(db).to_string()),
        typed_exprs: thir.as_ref().map_or(vec![], |thir| {
            thir.node_types(db)
                .into_iter()
                .map(|(key, val)| (key, val.display(db).to_string()))
                .collect()
        }),
        diagnostics: thir.as_ref().map_or(vec![], |thir| {
            thir.diagnostics(db)
                .into_iter()
                .filter_map(|diag| {
                    println!("TODO: report diagnostics: {diag:?}");
                    None
                })
                .collect()
        }),
    });
}

pub fn check_definition<'a>(
    db: &'a dyn Db,
    def: Definition,
    package: Package<'a>,
    packages: &[Package<'a>],
    report: &mut Report,
) {
    match def {
        Definition::Function(function_id) => check_function(db, function_id, packages, report),
        Definition::Interface(interface_id) => {
            check_interface(db, interface_id, package, packages, report)
        }
        Definition::Module(module) => check_module(db, module, package, packages, report),
        Definition::Type(type_def_id) => check_type_def(db, type_def_id, package, packages, report),
    }
}

pub fn check_implem<'a>(
    db: &'a dyn Db,
    implem: ImplSource<'a>,
    packages: &[Package<'a>],
    report: &mut Report,
) {
    for item in implem.items(db) {
        match item {
            AstImplItem::Type { .. } => {
                if let Some(_interface_ref) = implem.id(db).interface(db) {
                    println!("Warning: not checking if types inside impl are well conforming")
                    // todo!("Check that the type conforms to everything as it should")
                }
            }
            AstImplItem::Fundef(spanned) => {
                let function_id =
                    FunctionId::new(db, spanned.data.name, ScopeOwnerId::Impl(implem.id(db)));
                check_function(db, function_id, packages, report);
            }
        }
    }
}

pub fn check_module<'a>(
    db: &'a dyn Db,
    module: ModuleId,
    package: Package<'a>,
    packages: &[Package<'a>],
    report: &mut Report,
) {
    module_definitions(db, module.interned())
        .into_values()
        .for_each(|def| check_definition(db, def, package, packages, report));

    module_impls(db, module.interned())
        .into_iter()
        .for_each(|implem| check_implem(db, implem, packages, report));

    module.file_submodules(db).iter().for_each(|submodule| {
        check_file_module(db, *submodule, Some(module), package, packages, report)
    });
}

pub fn parse_error_to_diagnostic(db: &dyn Db, parse_error: &ParseError) -> Diagnostic {
    todo!()
}

pub fn check_file_module<'a>(
    db: &'a dyn Db,
    file_module: FileModule<'a>,
    parent: Option<ModuleId>,
    package: Package<'a>,
    packages: &[Package<'a>],
    report: &mut Report,
) {
    let module = file_module_id(db, file_module, parent, package);
    let parse_errors: Vec<&ParseError> =
        parse_file::accumulated::<ParseError>(db, file_module.file(db));
    report.diagnostics.extend(
        parse_errors
            .into_iter()
            .map(|parse_error| parse_error_to_diagnostic(db, parse_error)),
    );
    check_module(db, module, package, packages, report);
}

pub fn check_package<'a>(
    db: &'a dyn Db,
    package: Package<'a>,
    packages: &[Package<'a>],
    report: &mut Report,
) {
    check_file_module(db, package.root(db), None, package, packages, report);
}

pub fn check(root: impl AsRef<Path>, config: Config) -> Result<Report, CompilerError> {
    let db = RockrDb::new(config);
    let root = root.as_ref();
    let package = driver::load_package(&db, root)
        .ok_or_else(|| CompilerError::NoCompilationUnitFound(root.to_path_buf()))?;
    let packages = build_package_set(&db, package)?;
    let mut report = Report::new(
        packages
            .iter()
            .map(|p| PackageInfo::from(&db, *p))
            .collect(),
    );

    packages
        .iter()
        .filter(|package| !db.config.skip_core || **package != core_package(&db))
        .for_each(|package| check_package(&db, *package, &packages, &mut report));

    Ok(report)
}
