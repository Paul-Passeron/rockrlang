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
    common::location::Location,
    lookup::enclosing_scope_owner,
    name_resolve::{
        definition::{Definition, resolve_in_module},
        interfaces::interface_item,
        module_items,
    },
    parse_tree::{
        expr::{AstExpr, AstExprDesc},
        pattern::{
            AstConstructFields, AstNamedPattern, AstPattern, AstPatternDesc,
            StructFieldPattern,
        },
        stmt::{AstMatchBranch, AstStmt, AstStmtDesc},
        top_level::{
            AstEnumDef, AstEnumVariant, AstEnumVariantKind, AstFundef, AstFundefArg,
            AstFunsig, AstImplItem, AstInterfaceItem, AstMethodDef, AstMethodsig,
            AstStructDef, AstTemplateArg, AstTopLevelItemDesc,
        },
        type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    },
    ril::{ImplId, InterfaceId, ModuleId, ScopeOwnerId},
};

pub fn path_node_at(db: &dyn Db, loc: Location) -> Option<Definition> {
    let scope_owner = enclosing_scope_owner(db, loc)?;
    match scope_owner {
        ScopeOwnerId::Module(module) => module_path_node_at(db, module, loc),
        ScopeOwnerId::Impl(impl_id) => impl_path_node_at(db, impl_id, loc),
        ScopeOwnerId::Interface(interface_ref) => {
            interface_path_node_at(db, interface_ref.def(db), loc)
        }
    }
}

fn enclosing_module(db: &dyn Db, loc: Location) -> Option<ModuleId> {
    match enclosing_scope_owner(db, loc)? {
        ScopeOwnerId::Module(module_id) => Some(module_id),
        ScopeOwnerId::Impl(owner) => Some(owner.parent(db)),
        ScopeOwnerId::Interface(owner) => Some(owner.def(db).parent(db)),
    }
}

fn module_path_node_at(
    db: &dyn Db,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    module_items(db, module.into()).as_ref()?.iter().find_map(|item| {
        item.span.encloses(loc).then_some(())?;
        match &item.data {
            // We should not be here, the enclosing scope owner was `module`, not another
            // nested scope owner
            AstTopLevelItemDesc::Module(_)
            | AstTopLevelItemDesc::Interface(_)
            | AstTopLevelItemDesc::Impl(_) => None,
            AstTopLevelItemDesc::Error(_) => None,
            AstTopLevelItemDesc::Fundef(fundef) => fundef_path_node_at(db, fundef, loc),
            AstTopLevelItemDesc::StructDef(struct_def) => {
                struct_path_node_at(db, struct_def, loc)
            }
            AstTopLevelItemDesc::EnumDef(enum_def) => {
                enum_path_node_at(db, enum_def, loc)
            }
            AstTopLevelItemDesc::ExternDef(extern_def, _) => {
                sig_path_node_at(db, extern_def, loc)
            }
        }
    })
}

fn fundef_path_node_at(
    db: &dyn Db,
    ast: &AstFundef,
    loc: Location,
) -> Option<Definition> {
    if ast.data.body_span.encloses(loc) {
        ast.data.body.iter().find_map(|stmt| stmt_path_node_at(db, stmt, loc))
    } else {
        ast.data
            .args
            .iter()
            .find_map(|arg| fundef_arg_path_node_at(db, arg, loc))
            .or_else(|| {
                ast.data
                    .template_args
                    .iter()
                    .find_map(|templ| template_arg_path_node_at(db, templ, loc))
            })
            .or_else(|| {
                type_expr_path_node_at(
                    db,
                    &ast.data.return_type,
                    enclosing_module(db, loc)?,
                    loc,
                )
            })
    }
}

fn template_arg_path_node_at(
    db: &dyn Db,
    ast: &AstTemplateArg,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    let module = enclosing_module(db, loc)?;
    ast.constraints.iter().find_map(|cons| type_expr_path_node_at(db, cons, module, loc))
}

fn fundef_arg_path_node_at(
    db: &dyn Db,
    ast: &AstFundefArg,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    let module = enclosing_module(db, loc)?;
    type_expr_path_node_at(db, &ast.ty, module, loc)
}

fn stmt_path_node_at(db: &dyn Db, ast: &AstStmt, loc: Location) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    let module = enclosing_module(db, loc)?;
    match &ast.data {
        AstStmtDesc::Return { value } => {
            value.as_ref().and_then(|expr| expr_path_node_at(db, expr, module, loc))
        }
        AstStmtDesc::If { cond, then, else_ } => expr_path_node_at(db, cond, module, loc)
            .or_else(|| stmt_path_node_at(db, then, loc))
            .or_else(|| {
                else_.as_ref().and_then(|else_| stmt_path_node_at(db, else_, loc))
            }),
        AstStmtDesc::While { cond, body } => expr_path_node_at(db, cond, module, loc)
            .or_else(|| stmt_path_node_at(db, body, loc)),
        AstStmtDesc::For { element, iterator, body } => {
            pat_path_node_at(db, element, module, loc)
                .or_else(|| expr_path_node_at(db, iterator, module, loc))
                .or_else(|| stmt_path_node_at(db, body, loc))
        }
        AstStmtDesc::LetDecl { pat, type_constraint, value } => {
            pat_path_node_at(db, pat, module, loc)
                .or_else(|| {
                    type_constraint
                        .as_ref()
                        .and_then(|any_ty| any_type_path_node_at(db, any_ty, module, loc))
                })
                .or_else(|| expr_path_node_at(db, value, module, loc))
        }
        AstStmtDesc::Block { stmts } => {
            stmts.iter().find_map(|stmt| stmt_path_node_at(db, stmt, loc))
        }
        AstStmtDesc::Assign { lhs, rhs }
        | AstStmtDesc::CompoundAssign { lhs, rhs, .. } => {
            expr_path_node_at(db, lhs, module, loc)
                .or_else(|| expr_path_node_at(db, rhs, module, loc))
        }
        AstStmtDesc::Match { scrutinee, branches } => {
            expr_path_node_at(db, scrutinee, module, loc).or_else(|| {
                branches.iter().find_map(|br| branch_path_node_at(db, br, module, loc))
            })
        }
        AstStmtDesc::Break => None,
        AstStmtDesc::Expr(expr) => expr_path_node_at(db, expr, module, loc),
        AstStmtDesc::Defer(stmt) => stmt_path_node_at(db, stmt, loc),
        AstStmtDesc::Error(_) => None,
    }
}

fn expr_path_node_at(
    db: &dyn Db,
    ast: &AstExpr,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    match &ast.data {
        AstExprDesc::NameResolved { from, to } => {
            let resolved = resolve_in_module(db, from.data, module)?;
            if from.span.encloses(loc) {
                Some(resolved)
            } else {
                match resolved {
                    Definition::Module(resolved_module) => {
                        expr_path_node_at(db, to, resolved_module, loc)
                    }
                    _ => None,
                }
            }
        }
        AstExprDesc::StaticCall { ty, type_args, args, .. } => {
            if let Some(res) = type_expr_path_node_at(db, ty, module, loc) {
                Some(res)
            } else {
                let module = enclosing_module(db, loc)?;
                type_args
                    .iter()
                    .find_map(|ty| any_type_path_node_at(db, ty, module, loc))
                    .or_else(|| {
                        args.iter()
                            .find_map(|expr| expr_path_node_at(db, expr, module, loc))
                    })
            }
        }
        AstExprDesc::Index { object: a, index: b }
        | AstExprDesc::BinOp { lhs: a, rhs: b, .. }
        | AstExprDesc::Range { from: a, to: b } => expr_path_node_at(db, a, module, loc)
            .or_else(|| expr_path_node_at(db, b, module, loc)),
        AstExprDesc::FieldAccess { object: expr, .. }
        | AstExprDesc::TupleAccess { object: expr, .. }
        | AstExprDesc::Metadata(expr)
        | AstExprDesc::Ref(_, expr)
        | AstExprDesc::Neg(expr)
        | AstExprDesc::Not(expr)
        | AstExprDesc::AddressOf(expr)
        | AstExprDesc::PostfixDeref(expr)
        | AstExprDesc::PrefixDeref(expr) => expr_path_node_at(db, expr, module, loc),
        AstExprDesc::Call { callee: expr, type_args, args }
        | AstExprDesc::MethodCall { object: expr, type_args, args, .. } => {
            if let Some(def) = expr_path_node_at(db, expr, module, loc) {
                Some(def)
            } else {
                let module = enclosing_module(db, loc)?;
                type_args
                    .iter()
                    .find_map(|ty| any_type_path_node_at(db, ty, module, loc))
                    .or_else(|| {
                        args.iter()
                            .find_map(|expr| expr_path_node_at(db, expr, module, loc))
                    })
            }
        }
        AstExprDesc::StructLit { ty, fields, .. } => {
            type_expr_path_node_at(db, ty, module, loc).or_else(|| {
                let module = enclosing_module(db, loc)?;
                fields
                    .iter()
                    .find_map(|field| expr_path_node_at(db, &field.value, module, loc))
            })
        }
        AstExprDesc::Tuple(exprs) | AstExprDesc::SliceLit(exprs) => {
            exprs.iter().find_map(|expr| expr_path_node_at(db, expr, module, loc))
        }
        AstExprDesc::QualifiedPath { ty, .. } => {
            type_expr_path_node_at(db, ty, module, loc)
        }
        AstExprDesc::SizeOf(ty) | AstExprDesc::TypeName(ty) => {
            type_expr_path_node_at(db, ty, enclosing_module(db, loc)?, loc)
        }
        AstExprDesc::As { expr, ty } => {
            let module = enclosing_module(db, loc)?;
            expr_path_node_at(db, expr, module, loc)
                .or_else(|| type_expr_path_node_at(db, ty, module, loc))
        }
        _ => None,
    }
}

fn branch_path_node_at(
    db: &dyn Db,
    ast: &AstMatchBranch,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    let span = ast.pat.span.start().span(ast.body.span.end());
    span.encloses(loc).then_some(())?;
    pat_path_node_at(db, &ast.pat, module, loc)
        .or_else(|| {
            ast.guard.as_ref().and_then(|expr| expr_path_node_at(db, expr, module, loc))
        })
        .or_else(|| stmt_path_node_at(db, &ast.body, loc))
}

fn any_type_path_node_at(
    db: &dyn Db,
    ast: &AstAnyTypeExpr,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    match &ast.data {
        AstAnyTypeExprDesc::Any => None,
        AstAnyTypeExprDesc::Known(ast_type_expr_desc) => {
            type_expr_desc_path_node_at(db, ast_type_expr_desc, module, loc)
        }
    }
}

fn type_expr_path_node_at(
    db: &dyn Db,
    ast: &AstTypeExpr,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    type_expr_desc_path_node_at(db, &ast.data, module, loc)
}

fn type_expr_desc_path_node_at(
    db: &dyn Db,
    ast: &AstTypeExprDesc,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    match ast {
        AstTypeExprDesc::Tuple(tys) => tys.iter().find_map(|ty| {
            any_type_path_node_at(db, ty, enclosing_module(db, loc)?, loc)
        }),
        AstTypeExprDesc::Named { name, args } => args
            .iter()
            .find_map(|ty| any_type_path_node_at(db, ty, enclosing_module(db, loc)?, loc))
            .or_else(|| {
                name.span.encloses(loc).then_some(())?;
                resolve_in_module(db, name.data, module)
            }),
        AstTypeExprDesc::NameResolved { from, to } => {
            let resolved = resolve_in_module(db, from.data, module)?;
            if from.span.encloses(loc) {
                Some(resolved)
            } else {
                match resolved {
                    Definition::Module(resolved_module) => {
                        type_expr_path_node_at(db, to, resolved_module, loc)
                    }
                    _ => None,
                }
            }
        }
        AstTypeExprDesc::Ref { pointee: ty, .. }
        | AstTypeExprDesc::Pointer { pointee: ty, .. }
        | AstTypeExprDesc::Slice { ty, .. } => {
            any_type_path_node_at(db, ty, enclosing_module(db, loc)?, loc)
        }
        AstTypeExprDesc::Error(_) => None,
    }
}

fn named_pat_path_node_at(
    db: &dyn Db,
    ast: &AstNamedPattern,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    match ast {
        AstNamedPattern::Mut { .. } | AstNamedPattern::Bare(_) => None,
        AstNamedPattern::Constructor { args, .. } => cons_arg_node_at(db, args, loc),
        AstNamedPattern::NameResolved { from, to } => {
            let resolved = resolve_in_module(db, from.data, module)?;
            if from.span.encloses(loc) {
                Some(resolved)
            } else {
                match resolved {
                    Definition::Module(resolved_module) => {
                        named_pat_path_node_at(db, to, resolved_module, loc)
                    }
                    _ => None,
                }
            }
        }
        AstNamedPattern::Tuple { fields } => fields.iter().find_map(|field| {
            pat_path_node_at(db, field, enclosing_module(db, loc)?, loc)
        }),
    }
}

fn pat_path_node_at(
    db: &dyn Db,
    ast: &AstPattern,
    module: ModuleId,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    match &ast.data {
        AstPatternDesc::Named(named) => named_pat_path_node_at(db, named, module, loc),
        AstPatternDesc::IntLiteral(_)
        | AstPatternDesc::Any
        | AstPatternDesc::Error(_) => None,
    }
}

fn cons_arg_node_at(
    db: &dyn Db,
    ast: &AstConstructFields,
    loc: Location,
) -> Option<Definition> {
    match ast {
        AstConstructFields::TupleFields(fields) => fields.iter().find_map(|field| {
            pat_path_node_at(db, field, enclosing_module(db, loc)?, loc)
        }),
        AstConstructFields::StructFields(fields) => {
            fields.iter().find_map(|field| match field {
                StructFieldPattern::Rebind { pattern, .. } => {
                    pat_path_node_at(db, pattern, enclosing_module(db, loc)?, loc)
                }
                StructFieldPattern::Name(_, _) => None,
            })
        }
    }
}

fn struct_path_node_at(
    db: &dyn Db,
    ast: &AstStructDef,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    ast.template_args
        .iter()
        .find_map(|templ| template_arg_path_node_at(db, templ, loc))
        .or_else(|| {
            let module = enclosing_module(db, loc)?;
            ast.fields
                .iter()
                .find_map(|field| type_expr_path_node_at(db, &field.ty, module, loc))
        })
}

fn enum_path_node_at(db: &dyn Db, ast: &AstEnumDef, loc: Location) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    ast.template_args
        .iter()
        .find_map(|templ| template_arg_path_node_at(db, templ, loc))
        .or_else(|| {
            ast.variants
                .iter()
                .find_map(|variant| enum_variant_path_node_at(db, variant, loc))
        })
}

fn enum_variant_path_node_at(
    db: &dyn Db,
    ast: &AstEnumVariant,
    loc: Location,
) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    match &ast.kind {
        AstEnumVariantKind::Unit => None,
        AstEnumVariantKind::StructLike(fields) => {
            let module = enclosing_module(db, loc)?;
            fields
                .iter()
                .find_map(|field| type_expr_path_node_at(db, &field.ty, module, loc))
        }
        AstEnumVariantKind::TupleLike(fields) => {
            let module = enclosing_module(db, loc)?;
            fields.iter().find_map(|field| type_expr_path_node_at(db, field, module, loc))
        }
    }
}

fn sig_path_node_at(db: &dyn Db, ast: &AstFunsig, loc: Location) -> Option<Definition> {
    ast.span.encloses(loc).then_some(())?;
    ast.data
        .args
        .iter()
        .find_map(|arg| fundef_arg_path_node_at(db, arg, loc))
        .or_else(|| {
            ast.data
                .template_args
                .iter()
                .find_map(|templ| template_arg_path_node_at(db, templ, loc))
        })
        .or_else(|| {
            type_expr_path_node_at(
                db,
                &ast.data.return_type,
                enclosing_module(db, loc)?,
                loc,
            )
        })
}

fn impl_path_node_at(db: &dyn Db, impl_id: ImplId, loc: Location) -> Option<Definition> {
    let module = impl_id.parent(db);
    let ast =
        module_items(db, module.into()).as_ref()?.iter().find_map(|ast| {
            match &ast.data {
                AstTopLevelItemDesc::Impl(blk) => blk.span.encloses(loc).then_some(blk),
                _ => None,
            }
        })?;
    ast.template_args
        .iter()
        .find_map(|templ| template_arg_path_node_at(db, templ, loc))
        .or_else(|| type_expr_path_node_at(db, &ast.implemented, module, loc))
        .or_else(|| {
            ast.interface
                .as_ref()
                .and_then(|ast| type_expr_path_node_at(db, ast, module, loc))
        })
        .or_else(|| {
            ast.items.iter().find_map(|item| impl_item_node_path_at(db, item, loc))
        })
}

fn interface_path_node_at(
    db: &dyn Db,
    interface_id: InterfaceId,
    loc: Location,
) -> Option<Definition> {
    let ast = interface_item(db, interface_id.interned());
    let module = interface_id.parent(db);
    ast.template_args
        .iter()
        .find_map(|templ| template_arg_path_node_at(db, templ, loc))
        .or_else(|| {
            ast.supers.iter().find_map(|sup| type_expr_path_node_at(db, sup, module, loc))
        })
        .or_else(|| {
            ast.items.iter().find_map(|item| interface_item_path_node_at(db, item, loc))
        })
}

fn interface_item_path_node_at(
    db: &dyn Db,
    item: &AstInterfaceItem,
    loc: Location,
) -> Option<Definition> {
    match item {
        AstInterfaceItem::Type(ast) => template_arg_path_node_at(db, ast, loc),
        AstInterfaceItem::Sig(sig) => method_sig_path_node_at(db, sig, loc),
    }
}

fn impl_item_node_path_at(
    db: &dyn Db,
    item: &AstImplItem,
    loc: Location,
) -> Option<Definition> {
    let module = enclosing_module(db, loc)?;
    match item {
        AstImplItem::Type { ty, .. } => type_expr_path_node_at(db, ty, module, loc),
        AstImplItem::Fundef(method_def) => method_def_path_node_at(db, method_def, loc),
    }
}

fn method_def_path_node_at(
    db: &dyn Db,
    ast: &AstMethodDef,
    loc: Location,
) -> Option<Definition> {
    if ast.data.body_span.encloses(loc) {
        ast.data.body.iter().find_map(|stmt| stmt_path_node_at(db, stmt, loc))
    } else {
        ast.data
            .args
            .iter()
            .find_map(|arg| fundef_arg_path_node_at(db, arg, loc))
            .or_else(|| {
                ast.data
                    .template_args
                    .iter()
                    .find_map(|templ| template_arg_path_node_at(db, templ, loc))
            })
            .or_else(|| {
                type_expr_path_node_at(
                    db,
                    &ast.data.return_type,
                    enclosing_module(db, loc)?,
                    loc,
                )
            })
    }
}

fn method_sig_path_node_at(
    db: &dyn Db,
    ast: &AstMethodsig,
    loc: Location,
) -> Option<Definition> {
    ast.data
        .args
        .iter()
        .find_map(|arg| fundef_arg_path_node_at(db, arg, loc))
        .or_else(|| {
            ast.data
                .template_args
                .iter()
                .find_map(|templ| template_arg_path_node_at(db, templ, loc))
        })
        .or_else(|| {
            type_expr_path_node_at(
                db,
                &ast.data.return_type,
                enclosing_module(db, loc)?,
                loc,
            )
        })
}
