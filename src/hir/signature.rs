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
    Db,
    common::symbols::Symbol,
    hir::{Mutability, function_ast},
    name_resolve::type_expr::{get_templates_of_fun_only, templates_of_owner},
    parse_tree::top_level::{AstReceiver, AstTemplateArg},
    resolved::{
        BuiltinTypeId, InterfaceRef, InternedFunctionId, TypeDefId, TypeRef, ptr_of,
        ref_of,
    },
    typecheck::inference::{
        InferTy,
        implicit::{AsAstImplCtx, AstImplicitContext},
    },
};
use itertools::Itertools;
use std::{fmt, sync::Arc};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ZelfArg {
    mutability: Mutability,
    kind: ZelfKind,
}
impl ZelfArg {
    pub fn get_zelf_type_for(&self, db: &dyn Db, ty: InferTy) -> InferTy {
        match self.kind {
            ZelfKind::Zelf => ty,
            ZelfKind::RefZelf => InferTy::Adt {
                def: TypeDefId::Builtin(BuiltinTypeId::ref_(db, self.mutability)),
                fields: vec![ty],
            },
            ZelfKind::PtrZelf => InferTy::Adt {
                def: TypeDefId::Builtin(BuiltinTypeId::ptr(db, self.mutability)),
                fields: vec![ty],
            },
        }
    }

    pub fn as_type_ref_for(&self, db: &dyn Db, ty: TypeRef) -> TypeRef {
        match self.kind {
            ZelfKind::Zelf => ty,
            ZelfKind::RefZelf => ref_of(db, ty, self.mutability.is_mut()).into(),
            ZelfKind::PtrZelf => ptr_of(db, ty, self.mutability.is_mut()).into(),
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
    pub implicit_templates: Vec<Vec<InterfaceRef>>, /* Templates inherited
                                                     * from
                                                     * environment */
    pub added_templates: Vec<Vec<InterfaceRef>>, /* Templates for this
                                                  * function only */
    pub args: Vec<(Symbol, TypeRef)>,
    pub ret: TypeRef,
}

impl AstReceiver {
    pub fn as_zelf_arg(&self) -> Option<ZelfArg> {
        let mutability = match self {
            Self::None => {
                return None;
            }
            Self::MutZelf(_)
            | Self::MutRefZelf(_)
            | Self::MutPtrZelf(_) => Mutability::Mutable,
            _ => Mutability::Const,
        };
        let kind = match self {
            Self::None => {
                return None;
            }
            Self::Zelf(_) | Self::MutZelf(_) => ZelfKind::Zelf,
            Self::RefZelf(_) | Self::MutRefZelf(_) => ZelfKind::RefZelf,
            Self::PtrZelf(_) | Self::MutPtrZelf(_) => ZelfKind::PtrZelf,
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
) -> FunctionSignature {
    let function_templates: Arc<[AstTemplateArg]> =
        get_templates_of_fun_only(db, function_id).iter().cloned().collect();
    let ctx =
        AstImplicitContext::new(db, *function_id.parent(db), function_templates.clone())
            .unwrap();
    let added_templates: Vec<Vec<InterfaceRef>> = function_templates
        .iter()
        .map(|t| {
            t.constraints
                .iter()
                .filter_map(|constraint| ctx.resolve_interface(db, &constraint.data))
                .collect()
        })
        .collect();

    let implicit_templates: Vec<Vec<InterfaceRef>> =
        templates_of_owner(db, *function_id.parent(db))
            .iter()
            .map(|t| {
                t.constraints
                    .iter()
                    .filter_map(|constraint| ctx.resolve_interface(db, &constraint.data))
                    .collect_vec()
            })
            .collect_vec();
    let ast = function_ast(db, function_id);
    let zelf = ast.inner(db).receiver().and_then(|r| r.as_zelf_arg());
    let args: Vec<_> = ast
        .inner(db)
        .get_args()
        .iter()
        .map(|arg| (arg.name, ctx.resolve_err(db, &arg.ty.data)))
        .collect();
    let ret = ctx.resolve_err(db, &ast.inner(db).get_ret().data);
    FunctionSignature {
        name: *function_id.name(db),
        zelf,
        implicit_templates,
        added_templates,
        args,
        ret,
    }
}
