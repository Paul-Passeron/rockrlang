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
    common::symbols::Symbol,
    hir::{Mutability, function_ast},
    name_resolve::type_expr::{get_templates_of_fun_only, get_templates_of_owner},
    parse_tree::top_level::{AstReceiver, AstTemplateArg},
    ril::{BuiltinTypeId, InterfaceRef, InternedFunctionId, TypeDefId, TypeRef},
    thir::inference::{InferTy, implicit::AstImplicitContext},
};
use itertools::Itertools;
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

pub mod diagnostic;

#[salsa::input(singleton)]
pub struct Workspace {
    pub inner: Arc<Config>,
    pub files: Vec<Arc<SourceFile>>,
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

pub fn check(root: impl AsRef<Path>, config: Config) -> Result<(), CompilerError> {
    let db = RockrDb::new();
    let root = root.as_ref();
    todo!("Build workspace, populate the DB and run the checks")
}
