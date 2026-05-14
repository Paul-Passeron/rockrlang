use crate::{
    Db,
    common::location::Span,
    name_resolve::definition::{Definition, resolve_in_module},
    parse_tree::{
        top_level::AstTemplateArg,
        type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    },
    ril::{InternedModuleId, TypeId, TypeParamId, TypeRef, ptr_of, slice_of, tuple_of},
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
                resolve_type_expr(db, &**to, module.interned(), template_args)
            } else {
                TypeResolution::Error
            }
        }
        AstTypeExprDesc::Pointer(pointee) => {
            match resolve_type_expr(db, &*pointee, module, template_args) {
                TypeResolution::Type(pointee) => TypeResolution::Type(ptr_of(db, pointee).into()),
                _ => TypeResolution::Error,
            }
        }
        AstTypeExprDesc::Slice { ty, len } => {
            assert!(len.is_none(), "TODO: handle non value-type");
            match resolve_type_expr(db, &*ty, module, template_args) {
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
