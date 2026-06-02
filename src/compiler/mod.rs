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
    Db, RockrDb, SourceFile,
    check::check,
    common::symbols::Symbol,
    compiler::diagnostic::Diag,
    driver::{ANCHOR_FILE_NAME, read_source_file},
    hir::{Mutability, function_ast},
    name_resolve::type_expr::{get_templates_of_fun_only, get_templates_of_owner},
    parse_tree::top_level::{AstReceiver, AstTemplateArg},
    printer::render_diagnostics,
    ril::{
        BuiltinTypeId, FileModule, InterfaceRef, InternedFunctionId, Package, TypeDefId, TypeRef,
    },
    typecheck::inference::{InferTy, implicit::AstImplicitContext},
};
use dashmap::DashSet;
use itertools::Itertools;
use salsa::Setter;
use std::{fmt, hash::Hash, path::PathBuf, sync::Arc};
use walkdir::WalkDir;

pub mod diagnostic;

#[salsa::input(singleton)]
pub struct Workspace {
    pub config: Config,
    #[returns(ref)]
    pub files: DashSet<SourceFile>,
    #[returns(ref)]
    pub roots: DashSet<PackageRoot>,
}

#[salsa::input]
pub struct PackageRoot {
    pub name: String,
    pub file: SourceFile,
}

impl Workspace {
    pub fn initialize(db: &dyn Db, config: Config) -> Self {
        Self::new(db, config, DashSet::new(), DashSet::new())
    }

    pub fn add_file(self, db: &mut dyn Db, file: SourceFile) {
        let files = self.files(db).clone();
        if files.insert(file) {
            self.set_files(db).to(files);
        }
    }

    pub fn remove_file(self, db: &mut dyn Db, file: SourceFile) {
        let files = self.files(db).clone();
        if files.remove(&file).is_some() {
            self.set_files(db).to(files);
        }
    }

    pub fn add_package_root(self, db: &mut dyn Db, root: PackageRoot) {
        let roots = self.roots(db).clone();
        let new_name = root.name(db);
        let new_file = root.file(db);
        let ws = Workspace::get(db);
        for known in roots.iter() {
            if known.name(db) == new_name {
                if known.file(db) == new_file {
                    return;
                }
                // This should not happen, so we'll see what to do in this case
                return;
            }
        }
        roots.insert(root);
        ws.set_roots(db).to(roots);
    }

    pub fn to_string(self, db: &dyn Db) -> String {
        format!(
            "Workspace {{\n    config: {:?},\n    files:\n        {}\n}}",
            self.config(db),
            self.files(db)
                .iter()
                .sorted_by_key(|x| x.path(db))
                .map(|f| f.path(db).display().to_string())
                .join("\n        ")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub no_std: bool,
    pub skip_core: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            no_std: false,
            skip_core: false,
        }
    }
}

pub enum CompilerError {
    NoCompilationUnitFound(PathBuf),
    STDLibNotFound,
    NoFileFoundAt(PathBuf),
    CoreLibNotFound,
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
            CompilerError::NoFileFoundAt(path_buf) => {
                write!(f, "No file found at {}", path_buf.display())
            }
            CompilerError::CoreLibNotFound => {
                write!(f, "Core library (`core`) package not found.")
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ZelfArg {
    mutability: Mutability,
    kind: ZelfKind,
}
impl ZelfArg {
    pub fn get_zelf_type_for(&self, db: &dyn Db, ty: InferTy) -> InferTy {
        match self.kind {
            ZelfKind::Zelf => ty,
            ZelfKind::RefZelf => {
                if self.mutability.is_mut() {
                    InferTy::Adt {
                        def: TypeDefId::Builtin(BuiltinTypeId::mut_ref(db)),
                        fields: Box::new([ty]),
                    }
                } else {
                    InferTy::Adt {
                        def: TypeDefId::Builtin(BuiltinTypeId::ref_(db)),
                        fields: Box::new([ty]),
                    }
                }
            }
            ZelfKind::PtrZelf => {
                if self.mutability.is_mut() {
                    InferTy::Adt {
                        def: TypeDefId::Builtin(BuiltinTypeId::mut_ptr(db)),
                        fields: Box::new([ty]),
                    }
                } else {
                    InferTy::Adt {
                        def: TypeDefId::Builtin(BuiltinTypeId::ptr(db)),
                        fields: Box::new([ty]),
                    }
                }
            }
        }
    }
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
) -> Arc<FunctionSignature> {
    let function_templates: Arc<[AstTemplateArg]> =
        get_templates_of_fun_only(db, function_id).into();
    let ctx =
        AstImplicitContext::new(db, function_id.parent(db), function_templates.clone()).unwrap();
    let added_templates: Vec<Vec<InterfaceRef>> = function_templates
        .iter()
        .map(|t| {
            t.constraints
                .iter()
                .flat_map(|constraint| ctx.resolve_interface(db, &constraint.data))
                .collect()
        })
        .collect();

    let implicit_templates: Vec<Vec<InterfaceRef>> =
        get_templates_of_owner(db, function_id.parent(db))
            .iter()
            .map(|t| {
                t.constraints
                    .iter()
                    .flat_map(|constraint| ctx.resolve_interface(db, &constraint.data))
                    .collect_vec()
            })
            .collect_vec();
    let ast = function_ast(db, function_id);
    let zelf = ast.inner(db).receiver().and_then(|r| r.as_zelf_arg());
    let args: Vec<_> = ast
        .inner(db)
        .get_args()
        .iter()
        .map(|arg| {
            let ty = ctx.resolve(db, &arg.ty.data).unwrap_or(TypeRef::Error);
            (arg.name, ty)
        })
        .collect();
    let ret = ctx
        .resolve(db, &ast.inner(db).get_ret().data)
        .unwrap_or(TypeRef::Error);
    Arc::new(FunctionSignature {
        name: function_id.name(db),
        zelf,
        implicit_templates,
        added_templates,
        args,
        ret,
    })
}

fn add_package_root_from_disk(db: &mut dyn Db, root: PathBuf) -> Result<(), CompilerError> {
    let root = root
        .canonicalize()
        .map_err(|_| CompilerError::NoFileFoundAt(root))?;
    let path_to_file = if root.is_dir() {
        root.join(ANCHOR_FILE_NAME)
    } else {
        root.clone()
    };
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
    std::env::var(env)
        .map_err(|_| CompilerError::CoreLibNotFound)
        .map(|path| {
            PathBuf::from(path)
                .canonicalize()
                .map_err(|_| CompilerError::CoreLibNotFound)
        })
        .flatten()
}

fn compute_package_roots(db: &mut dyn Db, root: PathBuf) -> Result<(), CompilerError> {
    add_package_root_from_disk(db, root)?;
    add_package_root_from_disk(db, core_path()?)?;
    if !db.config().no_std {
        add_package_root_from_disk(db, std_path()?)?;
    }
    Ok(())
}

fn compute_all_files_from_roots(db: &mut dyn Db) -> Result<(), CompilerError> {
    fn walk(db: &mut dyn Db, p: PathBuf) -> Result<(), CompilerError> {
        if p.is_dir() {
            WalkDir::new(p.clone())
                .into_iter()
                .filter_map(Result::ok)
                .filter(|path| {
                    if path.path() == &p {
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
            read_source_file(db, &p).ok_or_else(|| CompilerError::NoFileFoundAt(p.clone()))?;
        }

        Ok(())
    }
    let ws = Workspace::get(db);
    for root in ws.roots(db).clone().iter() {
        let path = root.file(db).path(db).clone();
        walk(
            db,
            if path.is_file() && path.file_name().unwrap().to_str().unwrap() == ANCHOR_FILE_NAME {
                path.parent().unwrap().to_path_buf()
            } else {
                path
            },
        )?;
    }
    Ok(())
}

fn load_workspace_from_disk(root: PathBuf, config: Config) -> Result<RockrDb, CompilerError> {
    let mut db = RockrDb::new();
    Workspace::initialize(&mut db, config);
    compute_package_roots(&mut db, root)?;
    compute_all_files_from_roots(&mut db)?;
    Ok(db)
}

pub fn check_from_disk(root: PathBuf, config: Config) -> Result<(), CompilerError> {
    let db = load_workspace_from_disk(root, config)?;
    let ws = Workspace::get(&db);
    check(&db, ws);
    let diags: Vec<&Diag> = check::accumulated::<Diag>(&db, ws);
    render_diagnostics(&db, diags.into_iter());
    Ok(())
}
#[salsa::tracked]
pub fn is_file_direct_submodule_of_file<'db>(
    db: &'db dyn Db,
    parent: SourceFile,
    child: SourceFile,
) -> bool {
    if parent == child {
        return false;
    }
    let p_path = parent.path(db);
    if p_path.file_name().unwrap() != ANCHOR_FILE_NAME {
        return false;
    }
    let c_path = child.path(db);
    let parent_dir = p_path.parent().unwrap();
    let child_dir = c_path.parent().unwrap();

    if parent_dir == child_dir {
        return true;
    }

    if c_path.file_name().unwrap() == ANCHOR_FILE_NAME && child_dir.parent() == Some(parent_dir) {
        return true;
    }

    false
}

#[salsa::tracked]
pub fn submodules_of_file<'db>(db: &'db dyn Db, file: SourceFile) -> Vec<FileModule<'db>> {
    if file.path(db).file_name().unwrap() != ANCHOR_FILE_NAME {
        return vec![];
    }

    let ws = Workspace::get(db);

    ws.files(db)
        .iter()
        .map(|sf| *sf)
        .filter(|sf| is_file_direct_submodule_of_file(db, file, *sf))
        .map(|sf| FileModule::new(db, sf, submodules_of_file(db, sf)))
        .collect()
}

#[salsa::tracked]
pub fn package_of_root<'db>(db: &'db dyn Db, root: PackageRoot) -> Package<'db> {
    let file = root.file(db);
    Package::new(db, FileModule::new(db, file, submodules_of_file(db, file)))
}

#[salsa::tracked]
pub fn workspace_packages<'db>(db: &'db dyn Db, ws: Workspace) -> Arc<Vec<Package<'db>>> {
    Arc::new(
        ws.roots(db)
            .iter()
            .map(|root| package_of_root(db, *root))
            .collect(),
    )
}
