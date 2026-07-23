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
    Db, SourceFile,
    common::location::{Location, Span},
    hir::{FunctionLikeAst, function_ast},
    lookup::{enclosing_fun, enclosing_scope_owner},
    name_resolve::{interfaces::interface_item, module_items},
    parse_tree::{
        expr::{AstExpr, AstExprDesc},
        stmt::{AstMatchBranch, AstStmt, AstStmtDesc},
        top_level::{
            AstEnumDef, AstEnumVariantKind, AstFundefArg, AstFunsig, AstImplItem,
            AstInterfaceItem, AstStructDef, AstTemplateArg, AstTopLevelItemDesc,
        },
        type_expr::{AstAnyTypeExpr, AstTypeExpr, AstTypeExprDesc},
    },
    ril::{FunctionId, ImplId, InterfaceRef, ModuleId, ScopeOwnerId, TypeRef},
    thir::thir_body,
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeNode {
    pub ty: TypeRef,
    pub span: Span,
}

pub fn type_node_at(db: &dyn Db, loc: Location) -> Option<TypeNode> {
    #[salsa::tracked(returns(clone))]
    fn _aux(db: &dyn Db, sf: SourceFile, offset: usize) -> Option<TypeNode> {
        let loc = Location::new(sf, offset);
        if let Some(func) = enclosing_fun(db, loc) {
            return function_type_at(db, func, loc);
        }
        match enclosing_scope_owner(db, loc)? {
            ScopeOwnerId::Module(module_id) => module_type_at(db, module_id, loc),
            ScopeOwnerId::Impl(impl_id) => impl_type_at(db, impl_id, loc),
            ScopeOwnerId::Interface(iref) => interface_type_at(db, iref, loc),
        }
    }
    _aux(db, loc.file, loc.offset)
}

fn function_type_at(db: &dyn Db, func: FunctionId, loc: Location) -> Option<TypeNode> {
    let ast = function_ast(db, func.into()).inner(db);
    let (templates, args, ret_ty, body): (
        &Vec<AstTemplateArg>,
        &Vec<AstFundefArg>,
        &AstTypeExpr,
        &[AstStmt],
    ) = match &ast {
        FunctionLikeAst::Fundef(a) => (
            &a.data.template_args,
            &a.data.args,
            &a.data.return_type,
            a.data.body.as_slice(),
        ),
        FunctionLikeAst::Method(a) => (
            &a.data.template_args,
            &a.data.args,
            &a.data.return_type,
            a.data.body.as_slice(),
        ),
        FunctionLikeAst::ExternDef(a, _) => {
            (&a.data.template_args, &a.data.args, &a.data.return_type, &[])
        }
        FunctionLikeAst::TraitMethod(a) => {
            (&a.data.template_args, &a.data.args, &a.data.return_type, &[])
        }
    };
    let ctx =
        AstImplicitContext::new(db, func.parent(db), templates.iter().cloned().collect())
            .ok()?;

    let resolved = thir_body(db, func).and_then(|t| t.resolved_type_seed_at(db, loc));

    if let Some(node) =
        args.iter().find_map(|arg| type_expr_at(db, &ctx, &arg.ty, resolved, loc))
    {
        return Some(node);
    }
    if let Some(node) = type_expr_at(db, &ctx, ret_ty, resolved, loc) {
        return Some(node);
    }
    body.iter().find_map(|stmt| stmt_type_at(db, &ctx, stmt, resolved, loc))
}

fn stmt_type_at(
    db: &dyn Db,
    ctx: &AstImplicitContext,
    stmt: &AstStmt,
    resolved: Option<TypeRef>,
    loc: Location,
) -> Option<TypeNode> {
    stmt.span.encloses(loc).then_some(())?;
    match &stmt.data {
        AstStmtDesc::Return { value } => {
            value.as_ref().and_then(|v| expr_type_at(db, ctx, v, resolved, loc))
        }
        AstStmtDesc::If { cond, then, else_ } => {
            expr_type_at(db, ctx, cond, resolved, loc)
                .or_else(|| stmt_type_at(db, ctx, then, resolved, loc))
                .or_else(|| {
                    else_.as_ref().and_then(|e| stmt_type_at(db, ctx, e, resolved, loc))
                })
        }
        AstStmtDesc::While { cond, body } => expr_type_at(db, ctx, cond, resolved, loc)
            .or_else(|| stmt_type_at(db, ctx, body, resolved, loc)),
        AstStmtDesc::For { iterator, body, .. } => {
            expr_type_at(db, ctx, iterator, resolved, loc)
                .or_else(|| stmt_type_at(db, ctx, body, resolved, loc))
        }
        AstStmtDesc::LetDecl { type_constraint, value, .. } => type_constraint
            .as_ref()
            .and_then(|t| any_type_expr_at(db, ctx, t, resolved, loc))
            .or_else(|| expr_type_at(db, ctx, value, resolved, loc)),
        AstStmtDesc::Block { stmts } => {
            stmts.iter().find_map(|s| stmt_type_at(db, ctx, s, resolved, loc))
        }
        AstStmtDesc::Assign { lhs, rhs }
        | AstStmtDesc::CompoundAssign { lhs, rhs, .. } => {
            expr_type_at(db, ctx, lhs, resolved, loc)
                .or_else(|| expr_type_at(db, ctx, rhs, resolved, loc))
        }
        AstStmtDesc::Match { scrutinee, branches } => {
            expr_type_at(db, ctx, scrutinee, resolved, loc).or_else(|| {
                branches.iter().find_map(|b| branch_type_at(db, ctx, b, resolved, loc))
            })
        }
        AstStmtDesc::Expr(e) => expr_type_at(db, ctx, e, resolved, loc),
        AstStmtDesc::Defer(s) => stmt_type_at(db, ctx, s, resolved, loc),
        AstStmtDesc::Break | AstStmtDesc::Error(_) => None,
    }
}

fn branch_type_at(
    db: &dyn Db,
    ctx: &AstImplicitContext,
    branch: &AstMatchBranch,
    resolved: Option<TypeRef>,
    loc: Location,
) -> Option<TypeNode> {
    branch
        .guard
        .as_ref()
        .and_then(|g| expr_type_at(db, ctx, g, resolved, loc))
        .or_else(|| stmt_type_at(db, ctx, &branch.body, resolved, loc))
}

fn expr_type_at(
    db: &dyn Db,
    ctx: &AstImplicitContext,
    expr: &AstExpr,
    resolved: Option<TypeRef>,
    loc: Location,
) -> Option<TypeNode> {
    expr.span.encloses(loc).then_some(())?;
    match &expr.data {
        AstExprDesc::IntLit(_)
        | AstExprDesc::CharLit(_)
        | AstExprDesc::StrLit(_)
        | AstExprDesc::CStrLit(_)
        | AstExprDesc::BoolLit(_)
        | AstExprDesc::Name(_) => None,
        AstExprDesc::NameResolved { to, .. } => expr_type_at(db, ctx, to, resolved, loc),
        AstExprDesc::StaticCall { ty, type_args, args, .. } => {
            type_expr_at(db, ctx, ty, resolved, loc)
                .or_else(|| {
                    type_args
                        .iter()
                        .find_map(|t| any_type_expr_at(db, ctx, t, resolved, loc))
                })
                .or_else(|| {
                    args.iter().find_map(|a| expr_type_at(db, ctx, a, resolved, loc))
                })
        }
        AstExprDesc::QualifiedPath { ty, .. } => type_expr_at(db, ctx, ty, resolved, loc),
        AstExprDesc::BinOp { lhs, rhs, .. }
        | AstExprDesc::Range { from: lhs, to: rhs } => {
            expr_type_at(db, ctx, lhs, resolved, loc)
                .or_else(|| expr_type_at(db, ctx, rhs, resolved, loc))
        }
        AstExprDesc::Neg(e)
        | AstExprDesc::Not(e)
        | AstExprDesc::AddressOf(e)
        | AstExprDesc::PostfixDeref(e)
        | AstExprDesc::PrefixDeref(e)
        | AstExprDesc::Metadata(e)
        | AstExprDesc::Ref(_, e) => expr_type_at(db, ctx, e, resolved, loc),
        AstExprDesc::FieldAccess { object, .. }
        | AstExprDesc::TupleAccess { object, .. } => {
            expr_type_at(db, ctx, object, resolved, loc)
        }
        AstExprDesc::Call { callee, type_args, args } => {
            expr_type_at(db, ctx, callee, resolved, loc)
                .or_else(|| {
                    type_args
                        .iter()
                        .find_map(|t| any_type_expr_at(db, ctx, t, resolved, loc))
                })
                .or_else(|| {
                    args.iter().find_map(|a| expr_type_at(db, ctx, a, resolved, loc))
                })
        }
        AstExprDesc::MethodCall { object, type_args, args, .. } => {
            expr_type_at(db, ctx, object, resolved, loc)
                .or_else(|| {
                    type_args
                        .iter()
                        .find_map(|t| any_type_expr_at(db, ctx, t, resolved, loc))
                })
                .or_else(|| {
                    args.iter().find_map(|a| expr_type_at(db, ctx, a, resolved, loc))
                })
        }
        AstExprDesc::Index { object, index } => {
            expr_type_at(db, ctx, object, resolved, loc)
                .or_else(|| expr_type_at(db, ctx, index, resolved, loc))
        }
        AstExprDesc::StructLit { ty, fields, .. } => {
            type_expr_at(db, ctx, ty, resolved, loc).or_else(|| {
                fields.iter().find_map(|f| expr_type_at(db, ctx, &f.value, resolved, loc))
            })
        }
        AstExprDesc::Tuple(items) | AstExprDesc::SliceLit(items) => {
            items.iter().find_map(|e| expr_type_at(db, ctx, e, resolved, loc))
        }
        AstExprDesc::SizeOf(ty) | AstExprDesc::TypeName(ty) => {
            type_expr_at(db, ctx, ty, resolved, loc)
        }
        AstExprDesc::As { expr, ty } => expr_type_at(db, ctx, expr, resolved, loc)
            .or_else(|| type_expr_at(db, ctx, ty, resolved, loc)),
    }
}

fn module_type_at(db: &dyn Db, module_id: ModuleId, loc: Location) -> Option<TypeNode> {
    module_items(db, module_id.interned()).as_ref()?.iter().find_map(|item| {
        item.span.encloses(loc).then_some(())?;
        match &item.data {
            AstTopLevelItemDesc::StructDef(ast) => {
                struct_type_at(db, module_id, ast, loc)
            }
            AstTopLevelItemDesc::EnumDef(ast) => enum_type_at(db, module_id, ast, loc),
            AstTopLevelItemDesc::ExternDef(sig, _) => {
                extern_type_at(db, module_id, sig, loc)
            }
            _ => None,
        }
    })
}

fn struct_type_at(
    db: &dyn Db,
    module_id: ModuleId,
    ast: &AstStructDef,
    loc: Location,
) -> Option<TypeNode> {
    let ctx = AstImplicitContext::new(
        db,
        ScopeOwnerId::Module(module_id),
        ast.template_args.iter().cloned().collect(),
    )
    .ok()?;
    ast.fields.iter().find_map(|field| type_expr_at(db, &ctx, &field.ty, None, loc))
}

fn enum_type_at(
    db: &dyn Db,
    module_id: ModuleId,
    ast: &AstEnumDef,
    loc: Location,
) -> Option<TypeNode> {
    let ctx = AstImplicitContext::new(
        db,
        ScopeOwnerId::Module(module_id),
        ast.template_args.iter().cloned().collect(),
    )
    .ok()?;
    ast.variants.iter().find_map(|variant| {
        variant.span.encloses(loc).then_some(())?;
        match &variant.kind {
            AstEnumVariantKind::Unit => None,
            AstEnumVariantKind::StructLike(fields) => fields
                .iter()
                .find_map(|field| type_expr_at(db, &ctx, &field.ty, None, loc)),
            AstEnumVariantKind::TupleLike(tys) => {
                tys.iter().find_map(|ty| type_expr_at(db, &ctx, ty, None, loc))
            }
        }
    })
}

fn extern_type_at(
    db: &dyn Db,
    module_id: ModuleId,
    sig: &AstFunsig,
    loc: Location,
) -> Option<TypeNode> {
    let ctx = AstImplicitContext::new(
        db,
        ScopeOwnerId::Module(module_id),
        sig.data.template_args.iter().cloned().collect(),
    )
    .ok()?;
    sig.data
        .args
        .iter()
        .find_map(|arg| type_expr_at(db, &ctx, &arg.ty, None, loc))
        .or_else(|| type_expr_at(db, &ctx, &sig.data.return_type, None, loc))
}

fn impl_type_at(db: &dyn Db, impl_id: ImplId, loc: Location) -> Option<TypeNode> {
    let module_id = impl_id.parent(db);
    let block =
        module_items(db, module_id.interned()).as_ref()?.iter().find_map(|item| {
            match &item.data {
                AstTopLevelItemDesc::Impl(block) if block.span.encloses(loc) => {
                    Some(block.clone())
                }
                _ => None,
            }
        })?;
    let ctx =
        AstImplicitContext::new(db, ScopeOwnerId::Impl(impl_id), Arc::from([])).ok()?;

    if let Some(iface) = &block.interface
        && let Some(node) = type_expr_at(db, &ctx, iface, None, loc)
    {
        return Some(node);
    }
    if let Some(node) = type_expr_at(db, &ctx, &block.implemented, None, loc) {
        return Some(node);
    }
    block.items.iter().find_map(|item| match item {
        AstImplItem::Type { ty, .. } => type_expr_at(db, &ctx, ty, None, loc),
        AstImplItem::Fundef(_) => None,
    })
}

fn interface_type_at(db: &dyn Db, iref: InterfaceRef, loc: Location) -> Option<TypeNode> {
    let ast = interface_item(db, iref.def(db).interned());
    let ctx =
        AstImplicitContext::new(db, ScopeOwnerId::Interface(iref), Arc::from([])).ok()?;

    if let Some(node) =
        ast.supers.iter().find_map(|sup| type_expr_at(db, &ctx, sup, None, loc))
    {
        return Some(node);
    }
    ast.items.iter().find_map(|item| match item {
        AstInterfaceItem::Type(arg) => {
            arg.constraints.iter().find_map(|c| type_expr_at(db, &ctx, c, None, loc))
        }
        AstInterfaceItem::Sig(_) => None,
    })
}

fn type_expr_at(
    db: &dyn Db,
    ctx: &AstImplicitContext,
    ty: &AstTypeExpr,
    resolved: Option<TypeRef>,
    loc: Location,
) -> Option<TypeNode> {
    ty.span.encloses(loc).then_some(())?;
    if let Some(node) = type_expr_children_at(db, ctx, &ty.data, resolved, loc) {
        return Some(node);
    }
    let ty_ref = resolved
        .filter(|r| !matches!(r, TypeRef::Unknown | TypeRef::Error))
        .unwrap_or_else(|| ctx.resolve_err(db, &ty.data));
    Some(TypeNode { ty: ty_ref, span: ty.span })
}

fn any_type_expr_at(
    db: &dyn Db,
    ctx: &AstImplicitContext,
    any: &AstAnyTypeExpr,
    resolved: Option<TypeRef>,
    loc: Location,
) -> Option<TypeNode> {
    match any.as_known() {
        Some(ty) => type_expr_at(db, ctx, &ty, resolved, loc),
        None => {
            any.span.encloses(loc).then_some(())?;
            let ty =
                resolved.filter(|r| !matches!(r, TypeRef::Unknown | TypeRef::Error))?;
            Some(TypeNode { ty, span: any.span })
        }
    }
}

fn project(db: &dyn Db, resolved: Option<TypeRef>, idx: usize) -> Option<TypeRef> {
    resolved.and_then(|r| r.as_type_id()).and_then(|id| id.args(db).get(idx).copied())
}

fn type_expr_children_at(
    db: &dyn Db,
    ctx: &AstImplicitContext,
    desc: &AstTypeExprDesc,
    resolved: Option<TypeRef>,
    loc: Location,
) -> Option<TypeNode> {
    match desc {
        AstTypeExprDesc::Named { args, .. } => {
            args.iter().enumerate().find_map(|(i, arg)| {
                any_type_expr_at(db, ctx, arg, project(db, resolved, i), loc)
            })
        }
        AstTypeExprDesc::NameResolved { to, .. } => {
            type_expr_children_at(db, ctx, &to.data, resolved, loc)
        }
        AstTypeExprDesc::Ref { pointee, .. }
        | AstTypeExprDesc::Pointer { pointee, .. } => {
            any_type_expr_at(db, ctx, pointee, project(db, resolved, 0), loc)
        }
        AstTypeExprDesc::Slice { ty, .. } => {
            any_type_expr_at(db, ctx, ty, project(db, resolved, 0), loc)
        }
        AstTypeExprDesc::Tuple(tys) => tys.iter().enumerate().find_map(|(i, ty)| {
            any_type_expr_at(db, ctx, ty, project(db, resolved, i), loc)
        }),
        AstTypeExprDesc::Error(_) => None,
    }
}
