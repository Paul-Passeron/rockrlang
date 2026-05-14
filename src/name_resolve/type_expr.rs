use std::sync::Arc;

use crate::{
    Db,
    common::location::Span,
    hir::{FunctionLikeAst, function_ast, impl_sources},
    name_resolve::{
        definition::{Definition, resolve_in_module},
        module_items,
    },
    parse_tree::{
        top_level::{AstEnumDef, AstStructDef, AstTemplateArg, AstTopLevelItemDesc},
        type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    },
    ril::{
        InternedEnumId, InternedFunctionId, InternedModuleId, InternedStructId, ScopeOwnerId,
        TypeId, TypeParamId, TypeRef, ptr_of, ref_of, slice_of, tuple_of,
    },
};

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
) -> TypeResolution {
    match &any_type_expr.data {
        AstAnyTypeExprDesc::Any => TypeResolution::Infer,
        AstAnyTypeExprDesc::Known(desc) => {
            resolve_spanned_type_expr_desc(db, desc, &any_type_expr.span, module, template_args)
        }
    }
}

pub fn resolve_spanned_type_expr_desc<'db>(
    db: &'db dyn Db,
    type_expr: &'db AstTypeExprDesc,
    _span: &'db Span,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
) -> TypeResolution {
    match type_expr {
        AstTypeExprDesc::Named { name, args } => {
            if args.is_empty()
                && let Some(idx) = template_args.iter().position(|p| p.name == *name)
            {
                return TypeResolution::Type(TypeRef::Param(TypeParamId(idx)));
            }

            resolve_in_module(db, name.interned(), module).map_or(TypeResolution::Error, |def| {
                if let Definition::Type(type_def_id) = def {
                    let resolved_args = args
                        .iter()
                        .map(
                            |arg| match resolve_any_type_expr(db, arg, module, template_args) {
                                TypeResolution::Type(type_ref) => Some(type_ref),
                                _ => None,
                            },
                        )
                        .collect::<Option<Vec<_>>>();
                    match resolved_args {
                        Some(resolved_args) => {
                            TypeResolution::Type(TypeId::new(db, type_def_id, resolved_args).into())
                        }
                        None => TypeResolution::Error,
                    }
                } else {
                    TypeResolution::Error
                }
            })
        }
        AstTypeExprDesc::NameResolved { from, to } => {
            if let Some(Definition::Module(module)) = resolve_in_module(db, from.interned(), module)
            {
                resolve_type_expr(db, to, module.interned(), template_args)
            } else {
                TypeResolution::Error
            }
        }
        AstTypeExprDesc::Pointer { mutable, pointee } => {
            match resolve_type_expr(db, pointee, module, template_args) {
                TypeResolution::Type(pointee) => {
                    TypeResolution::Type(ptr_of(db, pointee, *mutable).into())
                }
                _ => TypeResolution::Error,
            }
        }
        AstTypeExprDesc::Ref { mutable, pointee } => {
            match resolve_type_expr(db, pointee, module, template_args) {
                TypeResolution::Type(pointee) => {
                    TypeResolution::Type(ref_of(db, pointee, *mutable).into())
                }
                _ => TypeResolution::Error,
            }
        }
        AstTypeExprDesc::Slice { ty, len } => {
            assert!(len.is_none(), "TODO: handle non value-type");
            match resolve_type_expr(db, ty, module, template_args) {
                TypeResolution::Type(elem) => TypeResolution::Type(slice_of(db, elem).into()),
                _ => TypeResolution::Infer,
            }
        }
        AstTypeExprDesc::Tuple(tys) => {
            let types = tys
                .iter()
                .map(
                    |ty| match resolve_type_expr(db, ty, module, template_args) {
                        TypeResolution::Type(type_ref) => Some(type_ref),
                        _ => None,
                    },
                )
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
) -> TypeResolution {
    resolve_spanned_type_expr_desc(db, &type_expr.data, &type_expr.span, module, template_args)
}

#[salsa::tracked]
pub fn struct_item<'db>(db: &'db dyn Db, struct_id: InternedStructId<'db>) -> Arc<AstStructDef> {
    for item in module_items(db, struct_id.parent(db).interned()).unwrap_or_default() {
        if let AstTopLevelItemDesc::StructDef(ast) = item.data
            && ast.name == struct_id.name(db)
        {
            return Arc::new(ast);
        }
    }
    unreachable!()
}

#[salsa::tracked]
pub fn enum_item<'db>(db: &'db dyn Db, enum_id: InternedEnumId<'db>) -> Arc<AstEnumDef> {
    for item in module_items(db, enum_id.parent(db).interned()).unwrap_or_default() {
        if let AstTopLevelItemDesc::EnumDef(ast) = item.data
            && ast.name == enum_id.name(db)
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
pub fn templates_of_enum<'db>(
    db: &'db dyn Db,
    enum_id: InternedEnumId<'db>,
) -> Arc<Vec<AstTemplateArg>> {
    Arc::new(enum_item(db, enum_id).template_args.clone())
}

#[salsa::tracked]
pub fn get_templates_of_fun<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Vec<AstTemplateArg> {
    let mut res = vec![];
    match function.parent(db) {
        ScopeOwnerId::Module(_) => (),
        ScopeOwnerId::Impl(impl_id) => {
            // Add templates from the impl_id, maybe
            let sources = impl_sources(db, impl_id.interned());
            res.extend(sources.into_iter().next().unwrap().templates(db));
        }
    }
    let ast = function_ast(db, function);
    match ast.inner(db) {
        FunctionLikeAst::ExternDef(sig, _) => res.extend(sig.data.template_args.clone()),
        FunctionLikeAst::Fundef(def) => {
            res.extend(def.data.template_args.clone());
        }
        FunctionLikeAst::Method(def) => res.extend(def.data.template_args.clone()),
    }
    res
}
