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

use std::sync::Arc;

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{FunctionLikeAst, function_ast, impl_sources},
    name_resolve::{
        definition::{Definition, resolve_in_module},
        interfaces::interface_item,
        module_items,
    },
    parse_tree::{
        top_level::{
            AstEnumDef, AstStructDef, AstTemplateArg, AstTopLevelItemDesc,
        },
        type_expr::{
            AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc,
        },
    },
    ril::{
        InternedEnumId, InternedFunctionId, InternedModuleId, InternedStructId,
        ScopeOwnerId, StructId, TypeDefId, TypeId, TypeParamId, TypeRef,
        ptr_of, ref_of, slice_of, tuple_of,
    },
};

// TODO: See if we can't use AstImplicitCtx here instead

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum TypeResolution {
    Error,
    Infer,
    Type(TypeRef),
}

pub fn resolve_any_type_expr<'db>(
    db: &'db dyn Db,
    any_type_expr: &'db AstAnyTypeExpr,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
    has_zelf: bool,
) -> TypeResolution {
    match &any_type_expr.data {
        AstAnyTypeExprDesc::Any => TypeResolution::Infer,
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
) -> TypeResolution {
    match type_expr {
        AstTypeExprDesc::Named { name, args } => {
            if args.is_empty() {
                if *name == Symbol::new(db, "Self") {
                    return TypeResolution::Type(TypeRef::Zelf);
                }
                if let Some(idx) =
                    template_args.iter().position(|p| p.name == *name)
                {
                    return TypeResolution::Type(TypeRef::Param(TypeParamId(
                        idx,
                    )));
                }
            }

            resolve_in_module(db, *name, module.into()).map_or(
                TypeResolution::Error,
                |def| {
                    if let Definition::Type(type_def_id) = def {
                        let resolved_args = args
                            .iter()
                            .map(|arg| {
                                match resolve_any_type_expr(
                                    db,
                                    arg,
                                    module,
                                    template_args,
                                    has_zelf,
                                ) {
                                    TypeResolution::Type(type_ref) => {
                                        Some(type_ref)
                                    }
                                    _ => None,
                                }
                            })
                            .collect::<Option<Vec<_>>>();
                        match resolved_args {
                            Some(resolved_args) => TypeResolution::Type(
                                TypeId::new(db, type_def_id, resolved_args)
                                    .into(),
                            ),
                            None => TypeResolution::Error,
                        }
                    } else {
                        TypeResolution::Error
                    }
                },
            )
        }
        AstTypeExprDesc::NameResolved { from, to } => {
            if let Some(Definition::Module(module)) =
                resolve_in_module(db, *from, module.into())
            {
                resolve_type_expr(
                    db,
                    to,
                    module.interned(),
                    template_args,
                    has_zelf,
                )
            } else {
                TypeResolution::Error
            }
        }
        AstTypeExprDesc::Pointer { mutable, pointee } => {
            match resolve_type_expr(
                db,
                pointee,
                module,
                template_args,
                has_zelf,
            ) {
                TypeResolution::Type(pointee) => {
                    TypeResolution::Type(ptr_of(db, pointee, *mutable).into())
                }
                _ => TypeResolution::Error,
            }
        }
        AstTypeExprDesc::Ref { mutable, pointee } => {
            match resolve_type_expr(
                db,
                pointee,
                module,
                template_args,
                has_zelf,
            ) {
                TypeResolution::Type(pointee) => {
                    TypeResolution::Type(ref_of(db, pointee, *mutable).into())
                }
                _ => TypeResolution::Error,
            }
        }
        AstTypeExprDesc::Slice { ty, len } => {
            assert!(len.is_none(), "TODO: handle non value-type");
            match resolve_type_expr(db, ty, module, template_args, has_zelf) {
                TypeResolution::Type(elem) => {
                    TypeResolution::Type(slice_of(db, elem).into())
                }
                _ => TypeResolution::Infer,
            }
        }
        AstTypeExprDesc::Tuple(tys) => {
            let types = tys
                .iter()
                .map(|ty| {
                    match resolve_type_expr(
                        db,
                        ty,
                        module,
                        template_args,
                        has_zelf,
                    ) {
                        TypeResolution::Type(type_ref) => Some(type_ref),
                        _ => None,
                    }
                })
                .collect::<Option<_>>();
            match types {
                Some(tys) => TypeResolution::Type(tuple_of(db, tys).into()),
                None => TypeResolution::Error,
            }
        }
    }
}

#[inline(always)]
pub fn resolve_type_expr<'db>(
    db: &'db dyn Db,
    type_expr: &'db AstTypeExpr,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
    has_zelf: bool,
) -> TypeResolution {
    resolve_type_expr_desc(db, &type_expr.data, module, template_args, has_zelf)
}

#[salsa::tracked]
pub fn struct_item<'db>(
    db: &'db dyn Db,
    struct_id: InternedStructId<'db>,
) -> Arc<AstStructDef> {
    for item in
        module_items(db, struct_id.parent(db).interned()).unwrap_or_default()
    {
        if let AstTopLevelItemDesc::StructDef(ast) = item.data
            && ast.name.data == struct_id.name(db)
        {
            return Arc::new(ast);
        }
    }
    unreachable!()
}

#[salsa::tracked]
pub fn enum_item<'db>(
    db: &'db dyn Db,
    enum_id: InternedEnumId<'db>,
) -> Arc<AstEnumDef> {
    for item in
        module_items(db, enum_id.parent(db).interned()).unwrap_or_default()
    {
        if let AstTopLevelItemDesc::EnumDef(ast) = item.data
            && ast.name.data == enum_id.name(db)
        {
            return Arc::new(ast);
        }
    }
    unreachable!()
}

#[salsa::tracked]
pub fn templates_of_struct<'db>(
    db: &'db dyn Db,
    struct_id: InternedStructId<'db>,
) -> Arc<Vec<AstTemplateArg>> {
    Arc::new(struct_item(db, struct_id).template_args.clone())
}

#[salsa::tracked]
impl StructId {
    pub fn field_names(self, db: &dyn Db) -> Arc<[Symbol]> {
        let item = struct_item(db, self.interned());
        item.fields.iter().map(|field| field.name).collect()
    }
}

pub fn get_template_param_count(db: &dyn Db, ty: TypeDefId) -> usize {
    match ty {
        TypeDefId::Builtin(builtin_type_id) => {
            builtin_type_id.template_count(db)
        }
        TypeDefId::Struct(struct_id) => {
            templates_of_struct(db, struct_id.interned()).len()
        }
        TypeDefId::Enum(enum_id) => {
            templates_of_enum(db, enum_id.interned()).len()
        }
    }
}

#[salsa::tracked]
pub fn templates_of_enum<'db>(
    db: &'db dyn Db,
    enum_id: InternedEnumId<'db>,
) -> Arc<Vec<AstTemplateArg>> {
    Arc::new(enum_item(db, enum_id).template_args.clone())
}

#[salsa::tracked]
pub fn get_templates_of_fun_only<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Box<[AstTemplateArg]> {
    let mut res = vec![];
    let ast = function_ast(db, function);
    match ast.inner(db) {
        FunctionLikeAst::ExternDef(sig, _) => {
            res.extend(sig.data.template_args.clone())
        }
        FunctionLikeAst::Fundef(def) => {
            res.extend(def.data.template_args.clone());
        }
        FunctionLikeAst::Method(def) => {
            res.extend(def.data.template_args.clone())
        }
        FunctionLikeAst::TraitMethod(sig) => {
            res.extend(sig.data.template_args.clone())
        }
    }
    res.into()
}

#[salsa::tracked]
pub fn get_templates_of_fun<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Arc<[AstTemplateArg]> {
    templates_of_owner(db, function.parent(db))
        .iter()
        .cloned()
        .chain(get_templates_of_fun_only(db, function))
        .collect()
}

pub fn templates_of_owner(
    db: &dyn Db,
    scope_owner: ScopeOwnerId,
) -> Arc<[AstTemplateArg]> {
    match scope_owner {
        ScopeOwnerId::Module(_) => Arc::new([]),
        ScopeOwnerId::Impl(impl_id) => impl_sources(db, impl_id.interned())
            .into_iter()
            .next()
            .unwrap()
            .templates(db)
            .iter()
            .cloned()
            .collect::<Arc<[_]>>(),
        ScopeOwnerId::Interface(interface_ref) => {
            interface_item(db, interface_ref.def(db).interned())
                .template_args
                .iter()
                .cloned()
                .collect()
        }
    }
}
