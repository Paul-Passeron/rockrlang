use salsa::Accumulator;

use crate::{
    Db,
    common::location::Span,
    compiler::{diagnostic::Diag, get_sig_of_function},
    hir::function_ast,
    ril::{BuiltinTypeId, TypeDefId, TypeId, TypeRef},
    thir::{ExprId, ExprKind, Thir, ThirConstructorArgs, stmt::ThirStmt},
};

pub fn check_return(db: &dyn Db, thir: &Thir) {
    let ret_ty = thir.get_ret_ty(db);
    if ret_ty == get_never_ty(db) || ret_ty == get_void_ty(db) {
        return;
    }
    match check_stmts(db, thir, &thir.root) {
        Completeness::AlwaysReturns => (),
        Completeness::MayFallthrough { span } => Diag::todo(
            format!(
                "Emit real fallthrough diagnostic ({}:{})",
                file!(),
                line!()
            ),
            span,
        )
        .accumulate(db),
    }
}

fn get_never_ty(db: &dyn Db) -> TypeRef {
    TypeRef::Concrete(TypeId::new(
        db,
        TypeDefId::Builtin(BuiltinTypeId::never(db)),
        vec![],
    ))
}

fn get_void_ty(db: &dyn Db) -> TypeRef {
    TypeRef::Concrete(TypeId::new(
        db,
        TypeDefId::Builtin(BuiltinTypeId::void(db)),
        vec![],
    ))
}

pub fn expr_contains_never(db: &dyn Db, thir: &Thir, expr: ExprId) -> bool {
    let thir_expr = &thir.exprs[expr];
    if thir_expr.ty == get_never_ty(db) {
        return true;
    }
    match &thir_expr.kind {
        ExprKind::BinOp { lhs, rhs, .. } => {
            expr_contains_never(db, thir, *lhs)
                || expr_contains_never(db, thir, *rhs)
        }
        ExprKind::StructLit { fields, .. } => fields
            .iter()
            .any(|item| expr_contains_never(db, thir, item.1)),
        ExprKind::Not(e) | ExprKind::Neg(e) => {
            expr_contains_never(db, thir, *e)
        }
        ExprKind::Call { args: items, .. }
        | ExprKind::Tuple(items)
        | ExprKind::SliceLit(items) => items
            .iter()
            .any(|item| expr_contains_never(db, thir, *item)),
        ExprKind::Constructor { args, .. } => match args {
            ThirConstructorArgs::Tuple(items) => items
                .iter()
                .any(|item| expr_contains_never(db, thir, *item)),
            ThirConstructorArgs::Struct(items) => items
                .iter()
                .any(|item| expr_contains_never(db, thir, item.1)),
            ThirConstructorArgs::None => false,
        },
        _ => false, // places don't need to be checked as any temporary expression was spilled already
    }
}

pub enum Completeness {
    AlwaysReturns,
    MayFallthrough { span: Span },
}

pub fn check_stmts(
    db: &dyn Db,
    thir: &Thir,
    stmts: &[ThirStmt],
) -> Completeness {
    for (i, stmt) in stmts.iter().enumerate() {
        if let Completeness::AlwaysReturns = check_stmt(db, thir, stmt) {
            todo!()
        }
    }

    let ast = function_ast(db, thir.id.interned()).inner(db);
    let span = ast.body_span().unwrap_or_else(|| thir.id.span(db));
    return Completeness::MayFallthrough { span };
}

fn check_stmt(db: &dyn Db, thir: &Thir, stmt: &ThirStmt) -> Completeness {
    todo!()
}

impl Thir {
    pub fn get_ret_ty(&self, db: &dyn Db) -> TypeRef {
        get_sig_of_function(db, self.id.interned()).ret
    }
}
