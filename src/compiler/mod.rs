use std::path::Path;
use std::sync::Arc;
use std::{fmt, path::PathBuf};

use itertools::Itertools;

use crate::common::location::Location;
use crate::common::symbols::Symbol;
use crate::compiler::diagnostic::Diagnostic;
use crate::hir::{Mutability, function_ast, hir_body};
use crate::name_resolve::definition::{Definition, get_module_pretty_name, module_definitions};
use crate::name_resolve::implems::module_impls;
use crate::name_resolve::type_expr::{get_templates_of_fun_only, get_templates_of_owner};
use crate::name_resolve::{core_package, file_module_id, std_package};
use crate::parse_tree::top_level::{AstImplItem, AstReceiver, AstTemplateArg};
use crate::parser::{ParseError, parse_file};
use crate::ril::display::{Display, RilDisplay};
use crate::ril::{
    FileModule, FunctionId, ImplSource, InterfaceId, InterfaceRef, InternedFunctionId, ModuleId,
    Package, ScopeOwnerId, TypeDefId, TypeRef,
};
use crate::thir::inference::implicit::{AstImplicitContext, ImplicitContext};
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
    _db: &'a dyn Db,
    _interface_id: InterfaceId,
    _package: Package<'a>,
    _packages: &[Package<'a>],
    _report: &mut Report,
) {
    // Is there anything to do here ?
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
                "{parent}::`impl{} {}{}`",
                if impl_id.templates(db).is_empty() {
                    String::new()
                } else {
                    format!(
                        " <{}>",
                        impl_id
                            .templates(db)
                            .iter()
                            .enumerate()
                            .map(|(i, t)| {
                                format!(
                                    "T{i}{}",
                                    if !t.is_empty() {
                                        format!(
                                            ": {}",
                                            t.iter()
                                                .map(|it| it.display(db).to_string())
                                                .collect_vec()
                                                .join(" + ")
                                        )
                                    } else {
                                        String::new()
                                    }
                                )
                            })
                            .collect_vec()
                            .join(", ")
                    )
                },
                match impl_id.interface(db) {
                    Some(interface_ref) => format!("{} for ", interface_ref.display(db)),
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ZelfArg {
    mutability: Mutability,
    kind: ZelfKind,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ZelfKind {
    Zelf,
    RefZelf,
    PtrZelf,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct FunctionSignature {
    pub name: Symbol,
    pub zelf: Option<ZelfArg>,
    pub implicit_templates: Vec<Vec<InterfaceRef>>, // Templates inherited from environment
    pub added_templates: Vec<Vec<InterfaceRef>>,    // Templates for this function only
    pub args: Vec<(Symbol, TypeRef)>,
    pub ret: TypeRef,
}

impl AstReceiver {
    pub fn as_zelf_arg(&self) -> Option<ZelfArg> {
        let mutability = match self {
            AstReceiver::None => {
                return None;
            }
            AstReceiver::MutZelf(_) | AstReceiver::MutRefZelf(_) | AstReceiver::MutPtrZelf(_) => {
                Mutability::Mutable
            }
            _ => Mutability::Const,
        };
        let kind = match self {
            AstReceiver::None => {
                return None;
            }
            AstReceiver::Zelf(_) | AstReceiver::MutZelf(_) => ZelfKind::Zelf,
            AstReceiver::RefZelf(_) | AstReceiver::MutRefZelf(_) => ZelfKind::RefZelf,
            AstReceiver::PtrZelf(_) | AstReceiver::MutPtrZelf(_) => ZelfKind::PtrZelf,
        };
        Some(ZelfArg { mutability, kind })
    }
}

impl fmt::Display for ZelfArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.mutability, self.kind) {
            (Mutability::Const, ZelfKind::Zelf) => write!(f, "self"),
            (Mutability::Const, ZelfKind::RefZelf) => write!(f, "&self"),
            (Mutability::Const, ZelfKind::PtrZelf) => write!(f, "*self"),
            (Mutability::Mutable, ZelfKind::Zelf) => write!(f, "mut self"),
            (Mutability::Mutable, ZelfKind::RefZelf) => write!(f, "&mut self"),
            (Mutability::Mutable, ZelfKind::PtrZelf) => write!(f, "*mut self"),
        }
    }
}

#[salsa::tracked]
pub fn get_sig_of_function(
    db: &dyn Db,
    function_id: InternedFunctionId<'_>,
) -> Option<Arc<FunctionSignature>> {
    let function_templates: Arc<[AstTemplateArg]> =
        get_templates_of_fun_only(db, function_id).into();
    let ctx =
        AstImplicitContext::new(db, function_id.parent(db), function_templates.clone()).ok()?;
    let added_templates: Vec<Vec<InterfaceRef>> = function_templates
        .iter()
        .map(|t| {
            t.constraints
                .iter()
                .map(|constraint| ctx.resolve_interface(db, &constraint.data))
                .collect::<Option<_>>()
        })
        .collect::<Option<_>>()?;

    let implicit_templates: Vec<Vec<InterfaceRef>> =
        get_templates_of_owner(db, function_id.parent(db))
            .iter()
            .map(|t| {
                t.constraints
                    .iter()
                    .map(|constraint| ctx.resolve_interface(db, &constraint.data))
                    .collect::<Option<_>>()
            })
            .collect::<Option<_>>()?;
    let ast = function_ast(db, function_id);
    let zelf = ast.inner(db).receiver().and_then(|r| r.as_zelf_arg());
    let args: Vec<_> = ast
        .inner(db)
        .get_args()
        .iter()
        .map(|arg| ctx.resolve(db, &arg.ty.data).map(|ty| (arg.name, ty)))
        .collect::<Option<_>>()?;
    let ret = ctx.resolve(db, &ast.inner(db).get_ret().data)?;
    Some(Arc::new(FunctionSignature {
        name: function_id.name(db),
        zelf,
        implicit_templates,
        added_templates,
        args,
        ret,
    }))
}

impl FunctionSignature {
    pub fn display<'a, 'b>(&'a self, db: &'b dyn Db) -> Display<'b, &'a Self> {
        Display { value: self, db }
    }
}

impl<'a, 'b> fmt::Display for Display<'b, &'a FunctionSignature> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this = self.value;
        let db = self.db;
        write!(f, "{}", this.name.display(db))?;
        if !this.added_templates.is_empty() || !this.implicit_templates.is_empty() {
            write!(f, "<")?;
            for (i, t) in this
                .implicit_templates
                .iter()
                .chain(&this.added_templates)
                .enumerate()
            {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "T{i}")?;
                if !t.is_empty() {
                    write!(
                        f,
                        ": {}",
                        t.iter()
                            .map(|c| c.display(db).to_string())
                            .collect_vec()
                            .join(" + ")
                    )?;
                }
            }
            write!(f, ">")?;
        }
        write!(f, "(")?;
        if let Some(arg) = this.zelf {
            write!(f, "{arg}")?;
            if !this.args.is_empty() {
                write!(f, ", ")?;
            }
        }
        write!(
            f,
            "{}): {}",
            this.args
                .iter()
                .map(|arg| format!("{}: {}", arg.0.display(db), arg.1.display(db)))
                .collect_vec()
                .join(", "),
            this.ret.display(db)
        )?;

        Ok(())
    }
}

pub fn get_pretty_function<'a>(db: &'a dyn Db, function_id: FunctionId) -> String {
    format!(
        "{}::{}",
        get_pretty_owner(db, function_id.parent(db)),
        get_sig_of_function(db, function_id.interned())
            .unwrap()
            .display(db)
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

pub fn parse_error_to_diagnostic(
    db: &dyn Db,
    parse_error: &ParseError,
    module: ModuleId,
) -> Diagnostic {
    let start = Location::new(parse_error.start, parse_error.file.clone());
    let end = Location::new(parse_error.end, parse_error.file.clone());
    let start_info = start.loc_info(db, module).unwrap();
    let end_info = end.loc_info(db, module).unwrap();
    println!(
        "Parsing error: {start_info} - {end_info}: {:?}",
        parse_error.kind
    );
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
            .map(|parse_error| parse_error_to_diagnostic(db, parse_error, module)),
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
