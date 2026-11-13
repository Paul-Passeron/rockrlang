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
    hir::{function_ast, impl_sources},
    name_resolve::{
        definition::{Definition, resolve_in_module},
        interfaces::interface_item,
        module_items,
    },
    model::{
        EnumId, InternedEnumId, InternedFunctionId, InternedModuleId, InternedStructId,
        ScopeOwnerId, StructId, TypeDefId, TypeId, TypeParamId, TypeRef, array_of,
        ptr_of, ref_of, slice_of, tuple_of,
    },
};
use itertools::Itertools;
use rockr_base::{Db, mutability::Mutability, symbol::Symbol};
use rockr_syntax::parse_tree::{
    top_level::{AstEnumDef, AstStructDef, AstTemplateArg, AstTopLevelItemDesc},
    type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
};
use std::sync::Arc;

pub fn resolve_any_type_expr<'db>(
    db: &'db dyn Db,
    any_type_expr: &'db AstAnyTypeExpr,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
    has_zelf: bool,
) -> TypeRef {
    match &any_type_expr.data {
        AstAnyTypeExprDesc::Any => TypeRef::Unknown,
        AstAnyTypeExprDesc::Known(desc) => {
            resolve_type_expr_desc(db, desc, module, template_args, has_zelf)
        }
    }
}

pub fn resolve_type_expr_desc<'db>(
    db: &'db dyn Db,
    type_expr: &'db AstTypeExprDesc,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
    has_zelf: bool,
) -> TypeRef {
    match type_expr {
        AstTypeExprDesc::Named { name, args } => {
            if args.is_empty() {
                if name.data.is(db, "Self") {
                    return TypeRef::Zelf;
                }
                if let Some(idx) = template_args.iter().position(|p| p.name == name.data)
                {
                    return TypeRef::Param(TypeParamId(idx));
                }
            }

            resolve_in_module(db, name.data, module.into()).map_or(
                TypeRef::Error,
                |def| {
                    if let Definition::Type(type_def_id) = def {
                        let resolved_args = args
                            .iter()
                            .map(|arg| {
                                resolve_any_type_expr(
                                    db,
                                    arg,
                                    module,
                                    template_args,
                                    has_zelf,
                                )
                            })
                            .collect_vec();
                        TypeId::new(db, type_def_id, resolved_args).into()
                    } else {
                        TypeRef::Error
                    }
                },
            )
        }
        AstTypeExprDesc::NameResolved { from, to } => {
            if let Some(Definition::Module(module)) =
                resolve_in_module(db, from.data, module.into())
            {
                resolve_type_expr(db, to, module.interned(), template_args, has_zelf)
            } else {
                TypeRef::Error
            }
        }
        AstTypeExprDesc::Pointer { mutable, pointee } => {
            let pointee = pointee.as_known().map_or(TypeRef::Unknown, |ty| {
                resolve_type_expr(db, &ty, module, template_args, has_zelf)
            });
            ptr_of::<TypeId>(db, pointee, *mutable).into()
        }
        AstTypeExprDesc::Ref { mutable, pointee } => {
            let pointee = pointee.as_known().map_or(TypeRef::Unknown, |ty| {
                resolve_type_expr(db, &ty, module, template_args, has_zelf)
            });
            ref_of::<TypeId>(db, pointee, *mutable).into()
        }
        AstTypeExprDesc::Array { elem: ty, len } => {
            let elem = ty.as_known().map_or(TypeRef::Unknown, |ty| {
                resolve_type_expr(db, &ty, module, template_args, has_zelf)
            });
            array_of::<TypeId>(db, elem, *len).into()
        }
        AstTypeExprDesc::Slice { elem: ty, mutable } => {
            let elem = ty.as_known().map_or(TypeRef::Unknown, |ty| {
                resolve_type_expr(db, &ty, module, template_args, has_zelf)
            });
            TypeRef::from(slice_of::<TypeId>(
                db,
                elem,
                if *mutable { Mutability::Mutable } else { Mutability::Const },
            ))
        }

        AstTypeExprDesc::Tuple(tys) => {
            let types = tys
                .iter()
                .map(|ty| {
                    ty.as_known().map_or(TypeRef::Unknown, |ty| {
                        resolve_type_expr(db, &ty, module, template_args, has_zelf)
                    })
                })
                .collect_vec();
            tuple_of::<TypeId>(db, types).into()
        }
        AstTypeExprDesc::Error(_) => TypeRef::Error,
    }
}

pub fn resolve_type_expr<'db>(
    db: &'db dyn Db,
    type_expr: &'db AstTypeExpr,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
    has_zelf: bool,
) -> TypeRef {
    resolve_type_expr_desc(db, &type_expr.data, module, template_args, has_zelf)
}

impl StructId {
    pub fn item(self, db: &dyn Db) -> &AstStructDef {
        struct_item(db, self.interned())
    }
}

impl EnumId {
    pub fn item(self, db: &dyn Db) -> &AstEnumDef {
        enum_item(db, self.interned())
    }
}

#[salsa::tracked]
pub fn struct_item<'db>(
    db: &'db dyn Db,
    struct_id: InternedStructId<'db>,
) -> AstStructDef {
    for item in
        module_items(db, struct_id.parent(db).interned()).as_ref().into_iter().flatten()
    {
        if let AstTopLevelItemDesc::StructDef(ast) = &item.data
            && ast.name.data == *struct_id.name(db)
        {
            return ast.clone();
        }
    }
    unreachable!()
}

#[salsa::tracked]
pub fn enum_item<'db>(db: &'db dyn Db, enum_id: InternedEnumId<'db>) -> AstEnumDef {
    for item in
        module_items(db, enum_id.parent(db).interned()).as_ref().into_iter().flatten()
    {
        if let AstTopLevelItemDesc::EnumDef(ast) = &item.data
            && ast.name.data == *enum_id.name(db)
        {
            return ast.clone();
        }
    }
    unreachable!()
}

#[salsa::tracked]
pub fn templates_of_struct<'db>(
    db: &'db dyn Db,
    struct_id: InternedStructId<'db>,
) -> Vec<AstTemplateArg> {
    struct_item(db, struct_id).template_args.clone()
}

#[salsa::tracked]
impl StructId {
    pub fn field_names(self, db: &dyn Db) -> Arc<[Symbol]> {
        let item = self.item(db);
        item.fields.iter().map(|field| field.name).collect()
    }
}

pub fn get_template_param_count(db: &dyn Db, ty: TypeDefId) -> usize {
    match ty {
        TypeDefId::Builtin(builtin_type_id) => builtin_type_id.template_count(db),
        TypeDefId::Struct(struct_id) => {
            templates_of_struct(db, struct_id.interned()).len()
        }
        TypeDefId::Enum(enum_id) => templates_of_enum(db, enum_id.interned()).len(),
    }
}

#[salsa::tracked(returns(deref))]
pub fn templates_of_enum<'db>(
    db: &'db dyn Db,
    enum_id: InternedEnumId<'db>,
) -> Vec<AstTemplateArg> {
    enum_item(db, enum_id).template_args.clone()
}

#[salsa::tracked(returns(deref))]
pub fn get_templates_of_fun_only<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Vec<AstTemplateArg> {
    let mut res = vec![];
    let ast = function_ast(db, function);
    res.extend(ast.inner(db).templates().iter().cloned());
    res
}

#[salsa::tracked(returns(deref))]
pub fn get_templates_of_fun<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Vec<AstTemplateArg> {
    templates_of_owner(db, *function.parent(db))
        .iter()
        .cloned()
        .chain(get_templates_of_fun_only(db, function).iter().cloned())
        .collect()
}

#[salsa::interned]
struct InternedScopeOwnerId {
    inner: ScopeOwnerId,
}

#[salsa::tracked(returns(deref))]
fn _templates_of_owner<'db>(
    db: &'db dyn Db,
    scope_owner: InternedScopeOwnerId<'db>,
) -> Vec<AstTemplateArg> {
    match *scope_owner.inner(db) {
        ScopeOwnerId::Module(_) => Vec::new(),
        ScopeOwnerId::Impl(impl_id) => impl_sources(db, impl_id.interned())
            .iter()
            .next()
            .expect("An impl id should not exist without impls in source")
            .templates(db)
            .clone(),
        ScopeOwnerId::Interface(interface_ref) => {
            interface_item(db, interface_ref.def(db).interned()).template_args
        }
    }
}

pub fn templates_of_owner(db: &dyn Db, scope_owner: ScopeOwnerId) -> &[AstTemplateArg] {
    _templates_of_owner(db, InternedScopeOwnerId::new(db, scope_owner))
}
