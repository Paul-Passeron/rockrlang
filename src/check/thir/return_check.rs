use salsa::Accumulator;

use crate::{
    Db,
    common::location::Span,
    compiler::{diagnostic::Diag, get_sig_of_function},
    hir::function_ast,
    ril::{BuiltinTypeId, TypeDefId, TypeId, TypeRef},
    thir::{
        ExprId, ExprKind, Thir, ThirConstructorArgs, ThirExprWithSetup,
        ThirMatchBranch,
        stmt::{StmtKind, ThirStmt},
    },
};

pub fn check_return(db: &dyn Db, thir: &Thir) {
    let ret_ty = thir.get_ret_ty(db);
    if ret_ty == get_never_ty(db) || ret_ty == get_void_ty(db) {
        return;
    }
    if let Completeness::MayFallthrough { span } =
        check_stmts(db, thir, &thir.root)
    {
        Diag::generic_error(
            format!(
                "Emit real fallthrough diagnostic ({}:{})",
                file!(),
                line!()
            ),
            span,
        )
        .accumulate(db)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    AlwaysReturns,
    MayFallthrough { span: Span },
}

fn get_thir_body_span(db: &dyn Db, thir: &Thir) -> Span {
    // We return the body span of the thir without the braces if possible.
    if let Some(fst) = thir.root.first()
        && let Some(lst) = thir.root.last()
    {
        fst.span.start().span(lst.span.end())
    } else {
        function_ast(db, thir.id.interned())
            .inner(db)
            .body_span()
            .unwrap_or_else(|| thir.id.span(db))
    }
}

pub fn check_stmts(
    db: &dyn Db,
    thir: &Thir,
    stmts: &[ThirStmt],
) -> Completeness {
    for (i, stmt) in stmts.iter().enumerate() {
        if check_stmt(db, thir, stmt).always_returns() {
            if i < stmts.len() - 1 {
                let next_stmt = &stmts[i + 1];
                Diag::todo(
                    format!("Have a real unreachable diagnostic"),
                    next_stmt.span,
                )
                .accumulate(db);
            }
            return Completeness::AlwaysReturns;
        }
    }

    Completeness::MayFallthrough {
        span: get_thir_body_span(db, thir),
    }
}

fn compute_if_stmt_completeness(
    then_check: Completeness,
    else_check: Option<Completeness>,
    whole_span: Span,
) -> Completeness {
    match (then_check, else_check) {
        (Completeness::AlwaysReturns, Some(Completeness::AlwaysReturns)) => {
            Completeness::AlwaysReturns
        }
        (
            Completeness::AlwaysReturns,
            Some(Completeness::MayFallthrough { span }),
        )
        | (
            Completeness::MayFallthrough { span },
            Some(Completeness::AlwaysReturns) | None,
        ) => Completeness::MayFallthrough { span },
        _ => Completeness::MayFallthrough { span: whole_span },
    }
}

fn check_expr_with_setup(
    db: &dyn Db,
    thir: &Thir,
    expr: &ThirExprWithSetup,
) -> Completeness {
    if check_stmts(db, thir, &expr.stmts).always_returns()
        || expr_contains_never(db, thir, expr.expr)
    {
        Completeness::AlwaysReturns
    } else {
        Completeness::MayFallthrough {
            span: thir.exprs[expr.expr].span,
        }
    }
}

fn check_stmt(db: &dyn Db, thir: &Thir, stmt: &ThirStmt) -> Completeness {
    match &stmt.kind {
        StmtKind::Block { stmts, .. } => check_stmts(db, thir, stmts),
        StmtKind::If {
            then, else_, cond, ..
        } => {
            let cond_check = check_expr_with_setup(db, thir, cond);
            let then_check = check_stmts(db, thir, then);
            let else_check =
                else_.as_ref().map(|else_| check_stmts(db, thir, else_));
            if cond_check.always_returns() {
                Completeness::AlwaysReturns
            } else {
                compute_if_stmt_completeness(then_check, else_check, stmt.span)
            }
        }
        StmtKind::While { body, cond, .. } => {
            // TODO: Try to do some constant propagation to see if a while loop
            // has its condition as true or false and deal with this additional
            // info
            let cond_check = check_expr_with_setup(db, thir, cond);
            let _ = check_stmts(db, thir, body);
            if cond_check.always_returns() {
                Completeness::AlwaysReturns
            } else {
                Completeness::MayFallthrough { span: stmt.span }
            }
        }
        StmtKind::Expr(expr)
        | StmtKind::Assign { rhs: expr, .. }
        | StmtKind::Let { init: expr, .. } => {
            if expr_contains_never(db, thir, *expr) {
                Completeness::AlwaysReturns
            } else {
                Completeness::MayFallthrough { span: stmt.span }
            }
        }
        StmtKind::Return(_) => Completeness::AlwaysReturns,

        StmtKind::Match {
            scrutinee,
            branches,
        } => {
            let scrutinee_check = check_expr_with_setup(db, thir, scrutinee);
            if branches
                .iter()
                .any(|br| check_branch(db, thir, br).always_returns())
                || scrutinee_check.always_returns()
            {
                Completeness::AlwaysReturns
            } else {
                Completeness::MayFallthrough { span: stmt.span }
            }
        }
        _ => Completeness::MayFallthrough { span: stmt.span },
    }
}

impl ThirMatchBranch {
    pub fn get_whole_span(&self, thir: &Thir) -> Span {
        let start = self.pattern.span.start();
        let end = if let Some(last) = self.body.last() {
            last.span.end()
        } else if let Some(guard) = &self.guard {
            thir.exprs[guard.expr].span.end()
        } else {
            self.pattern.span.end()
        };
        start.span(end)
    }
}

fn check_branch(
    db: &dyn Db,
    thir: &Thir,
    branch: &ThirMatchBranch,
) -> Completeness {
    let guard_returns = branch.guard.as_ref().map_or(false, |expr| {
        check_expr_with_setup(db, thir, expr).always_returns()
    });
    let body_check = check_stmts(db, thir, &branch.body);
    if guard_returns || body_check.always_returns() {
        Completeness::AlwaysReturns
    } else {
        Completeness::MayFallthrough {
            span: branch.get_whole_span(thir),
        }
    }
}

impl Thir {
    pub fn get_ret_ty(&self, db: &dyn Db) -> TypeRef {
        get_sig_of_function(db, self.id.interned()).ret
    }
}

impl Completeness {
    pub fn always_returns(self) -> bool {
        self == Completeness::AlwaysReturns
    }
}
